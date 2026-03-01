<div align="center">
  <strong>English</strong> | <a href="./README_zh.md">简体中文</a>
</div>

<br>

<h1 align="center">LarkStack-Linear</h1>

<p align="center">
  A Rust middleware that syncs <a href="https://linear.app/">Linear</a> events to <a href="https://larksuite.com/">Lark / Feishu</a> notifications.
  <br>
  Axum 0.8, Tokio, async all the way down.
</p>

<p align="center">
  <a href="https://github.com/your-username/LarkStack-Linear/actions"><img src="https://github.com/your-username/LarkStack-Linear/actions/workflows/ci.yml/badge.svg" alt="CI Status"></a>
  <img src="https://img.shields.io/badge/Rust-2021_Edition-orange.svg" alt="Rust Version">
  <img src="https://img.shields.io/badge/Deployment-Railway-black.svg" alt="Railway">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License">
</p>

<hr>

## Features

- **Group notifications** — When a Linear issue is created or updated, an interactive card (color-coded by priority) is posted to a Lark group chat. A 500 ms debounce window coalesces rapid-fire updates so you don't get spammed.

- **DM on assign** — Assigning an issue sends a private message to the assignee's Lark account, matched by email. No manual ID mapping needed.
- **Link previews** — Paste a `linear.app` URL in Lark and it unfurls into a summary card. Handles the `url_verification` challenge and fetches issue details via Linear's GraphQL API.
- **Webhook signature verification** — All Linear webhooks are validated with HMAC-SHA256. Lark event callbacks are verified too.

## Architecture

- `axum 0.8` + `tokio` + `reqwest 0.12`
- `POST /webhook` handles Linear events; `POST /lark/event` handles Lark callbacks. The two paths are fully separated.
- Lark logic is split into `cards.rs` (card builders, pure functions) and `bot.rs` (tenant token cache + HTTP client).

### Endpoints
| Method | Path | Purpose |
| :--- | :--- | :--- |
| `POST` | `/webhook` | Linear webhook receiver |
| `POST` | `/lark/event` | Lark event callback (challenge + link preview) |
| `GET`  | `/health` | Health check (returns `"ok"`) |

## Configuration

Set these environment variables before running:

<p align="center">
  <img src="./docs/images/linear-api-config.jpeg" width="600" alt="Linear API Configuration">
  <br>
  <sup><i>Webhook and API key settings in Linear's workspace settings.</i></sup>
</p>

| Variable | Required | Description |
| :--- | :---: | :--- |
| `LINEAR_WEBHOOK_SECRET` | ✅ | HMAC signature verification |
| `LINEAR_API_KEY` | Phase 3 | GraphQL API access for link previews |
| `LARK_WEBHOOK_URL` | ✅ | Group chat webhook URL |
| `LARK_APP_ID` | Phase 2 | Bot app ID (for tenant token) |
| `LARK_APP_SECRET` | Phase 2 | Bot app secret |
| `LARK_VERIFICATION_TOKEN`| Phase 3 | Lark event callback verification |
| `PORT` | ❌ | Defaults to `3000` |

## Deployment (Railway)

The repo includes a multi-stage `Dockerfile` sized for [Railway](https://railway.app/).

<p align="center">
  <img src="./docs/images/railway-vars.png" width="600" alt="Railway Variables Configuration">
  <br>
  <sup><i>Paste your environment variables into Railway and deploy.</i></sup>
</p>

## Local development

1. Create a private Lark group with a custom bot. Add a "Local Debug" webhook in Linear.
2. Run `ngrok http 3000` to get a public URL.
3. `cargo run`, then point the Linear webhook to `https://<YOUR_NGROK_URL>/webhook`.
4. Code quality is enforced by `prek` locally and `cargo clippy -- -D warnings` in CI.

## License

[MIT License](./LICENSE)
