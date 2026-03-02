<div align="center">
  <strong>English</strong> | <a href="./README_zh.md">简体中文</a>
</div>

<br>

<h1 align="center">LarkStack-Linear</h1>

<p align="center">
  Rust middleware that syncs <a href="https://linear.app/">Linear</a> events to <a href="https://larksuite.com/">Lark / Feishu</a> notifications.
  <br>
  Runs as a native server (Tokio) or a Cloudflare Worker (WASM).
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-Edition_2024-orange.svg" alt="Rust Version">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License">
</p>

<hr>

## Features

- **Group notifications** — Issue create / update posts an interactive card to a Lark group, color-coded by priority. Rapid-fire updates are debounced into a single message.
- **DM on assign** — Assigning an issue sends a private message to the assignee's Lark account, matched by email.
- **Link previews** — Paste a `linear.app` URL in Lark and it unfurls into a summary card via Linear's GraphQL API.
- **Webhook signature verification** — HMAC-SHA256 for Linear, token verification for Lark.

## Endpoints

| Method | Path | Purpose |
| :--- | :--- | :--- |
| `POST` | `/webhook` | Linear webhook receiver |
| `POST` | `/lark/event` | Lark event callback (challenge + link preview) |
| `GET`  | `/health` | Health check |

## Quick Start

```bash
export LINEAR_WEBHOOK_SECRET=your_secret
export LARK_WEBHOOK_URL=https://open.larksuite.com/open-apis/bot/v2/hook/xxx
cargo run
```

See [Configuration](./docs/getting-started/configuration.md) for the full environment variable reference.

## Deployment

| Platform | Guide |
| :--- | :--- |
| Railway / Docker | [docs/deploy/railway.md](./docs/deploy/railway.md) |
| Cloudflare Workers | [docs/deploy/cloudflare-workers.md](./docs/deploy/cloudflare-workers.md) |

## Local Development

1. Create a Lark test group with a custom bot. Add a webhook in Linear.
2. `ngrok http 3000` to get a public URL.
3. `cargo run`, point the Linear webhook to `https://<NGROK_URL>/webhook`.

## License

[MIT](./LICENSE)
