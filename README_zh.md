<div align="center">
  <a href="./README.md">English</a> | <strong>简体中文</strong>
</div>

<br>

<h1 align="center">LarkStack-Linear</h1>

<p align="center">
  Rust 写的中间件，把 <a href="https://linear.app/">Linear</a> 的事件同步到<a href="https://larksuite.com/">飞书</a>通知。
  <br>
  Axum 0.8 + Tokio，全程异步。
</p>

<p align="center">
  <a href="https://github.com/your-username/LarkStack-Linear/actions"><img src="https://github.com/your-username/LarkStack-Linear/actions/workflows/ci.yml/badge.svg" alt="CI Status"></a>
  <img src="https://img.shields.io/badge/Rust-2021_Edition-orange.svg" alt="Rust Version">
  <img src="https://img.shields.io/badge/Deployment-Railway-black.svg" alt="Railway">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License">
</p>

<hr>

## 功能

- **群聊通知** — Linear Issue 创建或更新时，往飞书群发一张按优先级配色的 Interactive Card（Urgent 红色、High 橙色）。内置 500 ms 防抖，连续更新会合并成一条，不会刷屏。

- **指派时私聊** — Issue 分配给某人后，通过邮箱匹配自动给对方发飞书私聊，不需要手动维护用户 ID 映射表。
- **链接预览** — 在飞书里粘贴 `linear.app` 链接，自动展开成摘要卡片。底层处理了飞书的 `url_verification` 握手，通过 Linear GraphQL API 拉取详情。
- **Webhook 签名校验** — Linear webhook 用 HMAC-SHA256 验签，飞书事件回调同样做了校验。

## 架构

- `axum 0.8` + `tokio` + `reqwest 0.12`
- `POST /webhook` 处理 Linear 事件，`POST /lark/event` 处理飞书回调，两条路径完全隔离。
- 飞书相关逻辑拆成 `cards.rs`（卡片构建，纯函数）和 `bot.rs`（Tenant Token 缓存 + HTTP 请求）。

### 路由
| Method | Path | 用途 |
| :--- | :--- | :--- |
| `POST` | `/webhook` | 接收 Linear Webhook |
| `POST` | `/lark/event` | 接收飞书事件回调 (Challenge 验证 + 链接预览) |
| `GET`  | `/health` | 健康检查 (返回 `"ok"`) |

## 环境变量

运行前设好以下变量（本地可以用 `.env`）：

<p align="center">
  <img src="./docs/images/linear-api-config.jpeg" width="600" alt="Linear API Configuration">
  <br>
  <sup><i>在 Linear Workspace Settings 里配置 Webhook 密钥和 API Key。</i></sup>
</p>

| 变量名 | 必填 | 说明 |
| :--- | :---: | :--- |
| `LINEAR_WEBHOOK_SECRET` | ✅ | Webhook HMAC 签名验证 |
| `LINEAR_API_KEY` | Phase 3 | GraphQL API，用于链接预览 |
| `LARK_WEBHOOK_URL` | ✅ | 群聊机器人 Webhook 地址 |
| `LARK_APP_ID` | Phase 2 | 飞书应用 App ID（获取 Tenant Token） |
| `LARK_APP_SECRET` | Phase 2 | 飞书应用密钥 |
| `LARK_VERIFICATION_TOKEN`| Phase 3 | 飞书事件回调验证 |
| `PORT` | ❌ | 监听端口，默认 `3000` |

## 部署 (Railway)

仓库自带多阶段 `Dockerfile`，适配 [Railway](https://railway.app/)。

<p align="center">
  <img src="./docs/images/railway-vars.png" width="600" alt="Railway Variables Configuration">
  <br>
  <sup><i>在 Railway 面板填好环境变量就能部署。</i></sup>
</p>

## 本地开发

1. 建一个飞书测试群，加个自定义 Bot。在 Linear 新建一个 "Local Debug" Webhook。
2. `ngrok http 3000` 拿到公网地址。
3. `cargo run`，把 ngrok 地址填进 Linear webhook。
4. 代码规范靠 `prek` 本地检查 + CI 里的 `cargo clippy -- -D warnings`。

## 许可证

[MIT License](./LICENSE)
