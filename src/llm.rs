//! Shared LLM surface handlers reused by `/v1/*`, Amp, and Droid routes.

use crate::claude::{
    analyze_claude_request, convert_claude_request, convert_openai_response, error_from_proxy,
    extract_anthropic_model, is_native_claude_model, merge_tool_result_blocks,
    validate_anthropic_headers,
};
use crate::error::Error;
use crate::gemini::{convert_gemini_request, convert_openai_to_gemini_response};
use crate::initiator::{
    RequestAnalysis, analyze_openai_chat_completions, analyze_openai_responses,
};
use crate::proxy::forward_response;
use crate::server::AppState;
use crate::token_counter::handle_count_tokens;
use axum::body::Bytes;
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use serde_json::Value;

fn analyze_openai_request(
    path: &str,
    method: &Method,
    body: &[u8],
    headers: &HeaderMap,
) -> Option<RequestAnalysis> {
    if *method != Method::POST {
        return None;
    }
    match path {
        "chat/completions" => Some(analyze_openai_chat_completions(body, Some(headers))),
        "responses" => Some(analyze_openai_responses(body, Some(headers))),
        _ => None,
    }
}

pub async fn handle_openai_passthrough(
    state: &AppState,
    method: Method,
    api_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    let content_type = headers.get("content-type").and_then(|v| v.to_str().ok());
    let query = query.map(|q| format!("?{}", q)).unwrap_or_default();
    let analysis = analyze_openai_request(api_path, &method, &body, headers);

    let resp = state
        .proxy
        .forward(
            &format!("/{}{}", api_path, query),
            method,
            body,
            content_type,
            analysis.map(|a| a.initiator),
            analysis.map(|a| a.is_vision).unwrap_or(false),
        )
        .await?;
    forward_response(resp).await
}

/// Model aliases: (substring_to_replace, replacement).
/// Applied in order; first match wins.
/// Add new entries here when Copilot removes or renames a model.
const ANTHROPIC_MODEL_ALIASES: &[(&str, &str)] = &[
    ("claude-opus-4.6", "claude-opus-4.7"),
    ("claude-opus-4-6", "claude-opus-4-7"),
];

/// Convert `budget_tokens` to a Copilot `output_config.effort` string.
fn budget_tokens_to_effort(budget_tokens: u64) -> &'static str {
    if budget_tokens < 4_000 {
        "low"
    } else if budget_tokens < 16_000 {
        "medium"
    } else if budget_tokens < 28_000 {
        "high"
    } else {
        "xhigh"
    }
}

/// Rewrite an Anthropic request body:
/// 1. Remap deprecated model aliases.
/// 2. Convert `thinking.type: "enabled"` + `budget_tokens` to
///    `thinking.type: "adaptive"` + `output_config.effort`, which is what
///    Copilot's adaptive-thinking models (e.g. claude-opus-4.7) require.
fn remap_anthropic_model(body: Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return body,
    };

    // 1. Model alias remapping.
    let mut model_changed = false;
    if let Some(model) = obj.get("model").and_then(|v| v.as_str()) {
        let mut remapped = model.to_string();
        for (from, to) in ANTHROPIC_MODEL_ALIASES {
            if remapped.contains(from) {
                remapped = remapped.replace(from, to);
                break;
            }
        }
        if remapped != model {
            tracing::debug!(
                target: "llm",
                old_model = model,
                new_model = %remapped,
                "remapping deprecated Anthropic model"
            );
            obj.insert("model".to_string(), serde_json::Value::String(remapped));
            model_changed = true;
        }
    }

    // 2. Thinking type conversion: "enabled" -> "adaptive" + output_config.effort.
    let mut thinking_changed = false;
    if let Some(thinking) = obj.get_mut("thinking").and_then(|v| v.as_object_mut()) {
        if thinking.get("type").and_then(|v| v.as_str()) == Some("enabled") {
            let budget = thinking
                .get("budget_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(10_000);
            thinking.insert("type".to_string(), serde_json::Value::String("adaptive".to_string()));
            thinking.remove("budget_tokens");
            let effort = budget_tokens_to_effort(budget);
            tracing::debug!(
                target: "llm",
                budget_tokens = budget,
                effort,
                "converting thinking.type enabled -> adaptive"
            );
            thinking_changed = true;
            // Insert output_config.effort (merge with existing if present).
            let effort_val = serde_json::json!({ "effort": effort });
            obj.entry("output_config".to_string())
                .and_modify(|v| {
                    if let Some(cfg) = v.as_object_mut() {
                        cfg.insert("effort".to_string(), serde_json::Value::String(effort.to_string()));
                    }
                })
                .or_insert(effort_val);
        }
    }

    if model_changed || thinking_changed {
        if let Ok(bytes) = serde_json::to_vec(&value) {
            return Bytes::from(bytes);
        }
    }
    body
}

pub async fn handle_anthropic_compat(
    state: &AppState,
    method: Method,
    api_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
    validate_client_api_key: bool,
) -> Result<Response, Error> {
    let body = remap_anthropic_model(body);
    let query = query.map(|q| format!("?{}", q)).unwrap_or_default();
    let content_type = headers.get("content-type").and_then(|v| v.to_str().ok());

    match api_path {
        "messages/count_tokens" => {
            if method != Method::POST {
                return Ok(error_from_proxy(Error::InvalidRequest(
                    "Only POST is supported for messages/count_tokens".to_string(),
                )));
            }
            if let Some(model) = extract_anthropic_model(&body)
                && is_native_claude_model(&model)
            {
                let resp = match state
                    .proxy
                    .forward(
                        &format!("/v1/messages/count_tokens{query}"),
                        method,
                        body,
                        content_type,
                        None,
                        false,
                    )
                    .await
                {
                    Ok(resp) => resp,
                    Err(err) => return Ok(error_from_proxy(err)),
                };
                return forward_response(resp).await;
            }
            handle_count_tokens(body).await
        }
        "messages" => {
            if method != Method::POST {
                return Ok(error_from_proxy(Error::InvalidRequest(
                    "Only POST is supported for messages".to_string(),
                )));
            }

            if validate_client_api_key && let Some(resp) = validate_anthropic_headers(headers) {
                return Ok(resp);
            }

            let metadata = match analyze_claude_request(&body, Some(headers)) {
                Ok(metadata) => metadata,
                Err(err) => return Ok(error_from_proxy(err)),
            };
            if is_native_claude_model(&metadata.model) {
                let forwarded_body = merge_tool_result_blocks(&body)
                    .map(Bytes::from)
                    .unwrap_or(body);
                let resp = match state
                    .proxy
                    .forward(
                        &format!("/v1/messages{}", query),
                        method,
                        forwarded_body,
                        content_type,
                        Some(&metadata.initiator),
                        metadata.is_vision,
                    )
                    .await
                {
                    Ok(resp) => resp,
                    Err(err) => return Ok(error_from_proxy(err)),
                };
                return forward_response(resp).await;
            }

            let converted = match convert_claude_request(body, Some(headers)) {
                Ok(converted) => converted,
                Err(err) => return Ok(error_from_proxy(err)),
            };
            let resp = match state
                .proxy
                .forward(
                    &format!("/chat/completions{}", query),
                    method,
                    converted.body,
                    Some("application/json"),
                    Some(&converted.initiator),
                    converted.is_vision,
                )
                .await
            {
                Ok(resp) => resp,
                Err(err) => return Ok(error_from_proxy(err)),
            };
            match convert_openai_response(resp, converted.model, converted.stream).await {
                Ok(response) => Ok(response),
                Err(err) => Ok(error_from_proxy(err)),
            }
        }
        _ => {
            handle_openai_passthrough(
                state,
                method,
                api_path,
                Some(query.trim_start_matches('?')),
                headers,
                body,
            )
            .await
        }
    }
}

pub async fn handle_gemini_generate_content(
    state: &AppState,
    method: Method,
    model: &str,
    query: Option<&str>,
    body: Bytes,
    stream: bool,
) -> Result<Response, Error> {
    let query = query.map(|q| format!("?{}", q)).unwrap_or_default();
    let converted = match convert_gemini_request(model, body, stream) {
        Ok(c) => c,
        Err(e) => return Ok(error_from_proxy(e)),
    };
    let resp = match state
        .proxy
        .forward(
            &format!("/chat/completions{}", query),
            method,
            converted.body,
            Some("application/json"),
            Some(converted.initiator),
            converted.is_vision,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return Ok(error_from_proxy(e)),
    };
    match convert_openai_to_gemini_response(resp, converted.model, converted.stream).await {
        Ok(r) => Ok(r),
        Err(e) => Ok(error_from_proxy(e)),
    }
}

pub fn extract_model_field(body: &[u8]) -> Result<String, Error> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| Error::InvalidRequest(format!("Invalid JSON body: {e}")))?;
    value
        .get("model")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| Error::InvalidRequest("Missing required field: model".to_string()))
}
