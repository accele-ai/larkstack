<div align="center">
  <strong>English</strong> | <a href="./README_zh.md">简体中文</a>
</div>

<br>

<h1 align="center">LarkStack-Linear </h1>

<p align="center">
  A high-performance, type-safe middleware written in <strong>Rust</strong> that integrates <a href="https://linear.app/">Linear</a> with <a href="https://larksuite.com/">Lark / Feishu</a>.
  <br>
  Built with Axum 0.8 & Tokio for zero-delay workspace integration.
</p>

<p align="center">
  <a href="https://github.com/your-username/LarkStack-Linear/actions"><img src="https://github.com/your-username/LarkStack-Linear/actions/workflows/ci.yml/badge.svg" alt="CI Status"></a>
  <img src="https://img.shields.io/badge/Rust-2021_Edition-orange.svg" alt="Rust Version">
  <img src="https://img.shields.io/badge/Deployment-Railway-black.svg" alt="Railway">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License">
</p>

<hr>

## ✨ Features

- 📢 **Group Notifications (Phase 1)**: Automatically pushes Interactive Cards to a designated Lark group when Linear Issues are created or updated. Cards are color-coded based on priority. Includes a **500ms DebounceMap** window to coalesce rapid-fire updates and prevent notification spam.

<p align="center">
  <img src="./docs/images/lark-update-card.png" width="600" alt="Lark Update Card Showcase">
  <br>
  <sup><i>Real-time Linear issue updates delivered to Lark via Interactive Cards.</i></sup>
</p>

- 👤 **Direct Message on Assign (Phase 2)**: Automatically sends a private DM to a team member when an issue is assigned to them. Matches the assignee's Linear email with their Lark account email natively—no manual ID mapping required!
- 🔗 **Rich Link Previews (Phase 3)**: When a user pastes a `linear.app` link in Lark, the bridge handles Lark's `url_verification` challenge, fetches issue details via Linear's GraphQL API, and unfurls the link into a detailed summary card.
- 🛡️ **Secure by Default**: Implements strict HMAC-SHA256 signature verification for Linear webhooks and natively handles Lark's Event callbacks.

## 🏗️ Architecture & Tech Stack

Following a robust refactoring, the codebase is highly modularized:
- **Framework**: `axum 0.8` + `tokio` (Async runtime) + `reqwest 0.12`.
- **Handlers**: Clean separation between `POST /webhook` (Linear) and `POST /lark/event` (Lark callbacks).
- **Lark Module**: Decoupled `cards.rs` (pure UI builders) and `bot.rs` (tenant token caching & HTTP client).

### API Endpoints
| Method | Path | Purpose |
| :--- | :--- | :--- |
| `POST` | `/webhook` | Linear webhook receiver |
| `POST` | `/lark/event` | Lark event callback (challenge + link preview) |
| `GET`  | `/health` | Health check (returns `"ok"`) |

## ⚙️ Configuration

Ensure the following environment variables are set. 

<p align="center">
  <img src="./docs/images/linear-api-config.jpeg" width="600" alt="Linear API Configuration">
  <br>
  <sup><i>Configure your Webhook and API Keys in the Linear Workspace Settings.</i></sup>
</p>

| Variable | Required | Description |
| :--- | :---: | :--- |
| `LINEAR_WEBHOOK_SECRET` | ✅ | Used for HMAC signature verification. |
| `LINEAR_API_KEY` | For Phase 3 | GraphQL API access for link previews. |
| `LARK_WEBHOOK_URL` | ✅ | Group notification target. |
| `LARK_APP_ID` | For Phase 2 | Bot app ID for tenant token. |
| `LARK_APP_SECRET` | For Phase 2 | Bot app secret. |
| `LARK_VERIFICATION_TOKEN`| For Phase 3 | Lark event callback verification. |
| `PORT` | ❌ | Defaults to `3000`. |

## 🚀 Deployment (Railway)

Optimized for [Railway](https://railway.app/) using a highly efficient multi-stage `Dockerfile` to keep the image size minimal and deployment times fast.

<p align="center">
  <img src="./docs/images/railway-vars.png" width="600" alt="Railway Variables Configuration">
  <br>
  <sup><i>Zero-config deployment: Just paste your environment variables into Railway.</i></sup>
</p>

## 💻 Local Development & Testing

1. **Create a Test Environment**: Set up a private Lark group with a new Custom Bot and create a "Local Debug" webhook in Linear.
2. **Start a Local Tunnel**: Expose your local server using `ngrok http 3000`.
3. **Run the Server**: Use `cargo run` and point your Linear webhook to `https://<YOUR_NGROK_URL>/webhook`.
4. **Code Quality**: This project uses `prek` (local `fmt`/`clippy` gatekeeper) and strict GitHub Actions (`cargo clippy -- -D warnings`) to enforce code standards.

## 📝 License

[MIT License](./LICENSE)