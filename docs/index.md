# LarkStack

Rust middleware that syncs [Linear](https://linear.app/) events to
[Lark / Feishu](https://larksuite.com/) notifications. Runs as a native server
(Tokio) or a Cloudflare Worker (WASM).

## What it does

| Feature | How it works |
| :--- | :--- |
| Group notifications | Issue create / update / comment → interactive card in a Lark group, color-coded by priority. Rapid-fire updates are debounced into one message. |
| DM on assign | Assigning an issue sends a private message to the assignee's Lark account, matched by email. |
| Link previews | Paste a `linear.app` URL in Lark → summary card via Linear's GraphQL API. |
| Webhook verification | HMAC-SHA256 for Linear, token verification for Lark. |

## Endpoints

| Method | Path | Purpose |
| :--- | :--- | :--- |
| `POST` | `/webhook` | Linear webhook receiver |
| `POST` | `/lark/event` | Lark event callback (challenge + link preview) |
| `GET`  | `/health` | Health check |

## Next steps

- [Quick start](getting-started/quickstart.md) — run LarkStack in under 5 minutes
- [Configuration](getting-started/configuration.md) — environment variables reference
- [Architecture](architecture.md) — how the codebase is organized
- Deploy to [Railway](deploy/railway.md) or [Cloudflare Workers](deploy/cloudflare-workers.md)
