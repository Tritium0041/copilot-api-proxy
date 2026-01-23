//! Initiator inference for sticky inference cost savings.
//!
//! This module implements "sticky inference": once a conversation has assistant/tool
//! messages, all subsequent requests are marked as agent-initiated and do not
//! consume Copilot premium requests.

use serde_json::Value;

/// Infer the initiator from Claude/Anthropic format message history.
/// Returns "agent" if any assistant/tool messages exist, "user" otherwise.
pub fn infer_initiator_claude(messages: &[Value]) -> &'static str {
    infer_initiator_from_messages(messages, &["assistant", "tool"])
}

/// Infer the initiator from OpenAI chat completions format message history.
/// Returns "agent" if any assistant/tool messages exist, "user" otherwise.
pub fn infer_initiator_openai_chat_completions(body: &[u8]) -> &'static str {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return "user";
    };
    let Some(messages) = value.get("messages").and_then(|v| v.as_array()) else {
        return "user";
    };
    infer_initiator_from_messages(messages, &["assistant", "tool"])
}

/// Infer the initiator from OpenAI responses API format.
/// The responses API uses "input" field with items that can have roles.
/// Returns "agent" if any assistant/tool items exist, "user" otherwise.
pub fn infer_initiator_openai_responses(body: &[u8]) -> &'static str {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return "user";
    };
    // Responses API uses "input" which can be a string or array of items
    let Some(input) = value.get("input") else {
        return "user";
    };
    // If input is a string, it's a simple user input
    if input.is_string() {
        return "user";
    }
    // If input is an array, check for assistant/tool roles
    let Some(items) = input.as_array() else {
        return "user";
    };
    infer_initiator_from_messages(items, &["assistant", "tool"])
}

fn infer_initiator_from_messages(messages: &[Value], agent_roles: &[&str]) -> &'static str {
    if messages.iter().any(|msg| {
        msg.get("role")
            .and_then(|v| v.as_str())
            .map(|r| agent_roles.contains(&r))
            .unwrap_or(false)
    }) {
        "agent"
    } else {
        "user"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_claude_user_only() {
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        assert_eq!(infer_initiator_claude(&messages), "user");
    }

    #[test]
    fn test_claude_with_assistant() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": "Hi there"}),
        ];
        assert_eq!(infer_initiator_claude(&messages), "agent");
    }

    #[test]
    fn test_claude_with_tool() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "tool", "content": "result"}),
        ];
        assert_eq!(infer_initiator_claude(&messages), "agent");
    }

    #[test]
    fn test_claude_multi_turn() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": "Hi"}),
            json!({"role": "user", "content": "How are you?"}),
        ];
        assert_eq!(infer_initiator_claude(&messages), "agent");
    }

    #[test]
    fn test_claude_empty() {
        let messages: Vec<Value> = vec![];
        assert_eq!(infer_initiator_claude(&messages), "user");
    }

    #[test]
    fn test_openai_user_only() {
        let body = json!({"messages": [{"role": "user", "content": "Hello"}]});
        assert_eq!(
            infer_initiator_openai_chat_completions(body.to_string().as_bytes()),
            "user"
        );
    }

    #[test]
    fn test_openai_with_assistant() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"}
            ]
        });
        assert_eq!(
            infer_initiator_openai_chat_completions(body.to_string().as_bytes()),
            "agent"
        );
    }

    #[test]
    fn test_openai_with_tool() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "tool", "tool_call_id": "123", "content": "result"}
            ]
        });
        assert_eq!(
            infer_initiator_openai_chat_completions(body.to_string().as_bytes()),
            "agent"
        );
    }

    #[test]
    fn test_openai_invalid_json() {
        assert_eq!(infer_initiator_openai_chat_completions(b"not json"), "user");
    }

    #[test]
    fn test_openai_missing_messages() {
        let body = json!({"model": "gpt-4"});
        assert_eq!(
            infer_initiator_openai_chat_completions(body.to_string().as_bytes()),
            "user"
        );
    }

    #[test]
    fn test_responses_string_input() {
        let body = json!({"input": "Hello, world!"});
        assert_eq!(
            infer_initiator_openai_responses(body.to_string().as_bytes()),
            "user"
        );
    }

    #[test]
    fn test_responses_user_only() {
        let body = json!({
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hello"}]}]
        });
        assert_eq!(
            infer_initiator_openai_responses(body.to_string().as_bytes()),
            "user"
        );
    }

    #[test]
    fn test_responses_with_assistant() {
        let body = json!({
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Hi"}]}
            ]
        });
        assert_eq!(
            infer_initiator_openai_responses(body.to_string().as_bytes()),
            "agent"
        );
    }

    #[test]
    fn test_responses_with_tool() {
        let body = json!({
            "input": [
                {"role": "user", "content": "Hello"},
                {"role": "tool", "content": "result"}
            ]
        });
        assert_eq!(
            infer_initiator_openai_responses(body.to_string().as_bytes()),
            "agent"
        );
    }

    #[test]
    fn test_responses_invalid_json() {
        assert_eq!(infer_initiator_openai_responses(b"not json"), "user");
    }

    #[test]
    fn test_responses_missing_input() {
        let body = json!({"model": "gpt-4"});
        assert_eq!(
            infer_initiator_openai_responses(body.to_string().as_bytes()),
            "user"
        );
    }
}
