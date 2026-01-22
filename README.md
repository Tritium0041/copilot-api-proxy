# copilot-api-proxy

A reverse proxy for the GitHub Copilot API that exposes OpenAI-compatible endpoints. It forwards requests unchanged and injects the required Copilot authentication headers.

> [!WARNING]
> This is a reverse-engineered proxy of GitHub Copilot API. It is not supported by GitHub, and may break unexpectedly. Use at your own risk.

> [!WARNING]
> **GitHub Security Notice:**
> Excessive automated or scripted use of Copilot (including rapid or bulk requests, such as via automated tools) may trigger GitHub's abuse-detection systems.
> You may receive a warning from GitHub Security, and further anomalous activity could result in temporary suspension of your Copilot access.
>
> GitHub prohibits use of their servers for excessive automated bulk activity or any activity that places undue burden on their infrastructure.
>
> Please review:
>
> - [GitHub Acceptable Use Policies](https://docs.github.com/site-policy/acceptable-use-policies/github-acceptable-use-policies#4-spam-and-inauthentic-activity-on-github)
> - [GitHub Copilot Terms](https://docs.github.com/site-policy/github-terms/github-terms-for-additional-products-and-features#github-copilot)
>
> Use this proxy responsibly to avoid account restrictions.

## Features

- OpenAI-compatible `/v1/*` endpoints
- Pure passthrough of request/response bodies (no schema translation).
- GitHub OAuth device flow for one-time authentication.
- Automatic Copilot token refresh in the background.
- Streaming responses are supported (SSE).

## Requirements

- A GitHub account with Copilot access

## Install & Build

```bash
cargo build
```

## Authenticate (one-time)

```bash
cargo run -- auth
```

This stores a GitHub token at `~/.local/share/copilot-api-proxy/github_token`.

## Run the Proxy

```bash
# Default port: 9876
cargo run -- server

# Custom port
cargo run -- server --port 8080
```

## Usage Examples

```bash
# Chat completions
curl -X POST http://localhost:9876/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o-mini-2024-07-18", "messages": [{"role": "user", "content": "Hello"}]}'

# Streaming
curl -X POST http://localhost:9876/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o-mini-2024-07-18", "messages": [{"role": "user", "content": "Hello"}], "stream": true}'

# Models
curl http://localhost:9876/v1/models

# Responses (gpt-5 only)
curl -X POST http://localhost:9876/v1/responses \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-5", "input": "Hello"}'
```

## Configuration

- `GITHUB_TOKEN` (optional): overrides the token file.
- `RUST_LOG`: control logging verbosity, for example:

```bash
RUST_LOG=copilot_api_proxy=debug,tower_http=debug cargo run -- server
```
