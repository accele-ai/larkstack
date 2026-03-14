<div align="center">
  <a href="./README.md">English</a> | <strong>简体中文</strong>
</div>

<br>

<h1 align="center">LarkStack-Linear</h1>

<p align="center">
  Rust 中间件，把 <a href="https://linear.app/">Linear</a> 事件同步到<a href="https://larksuite.com/">飞书</a>通知。
  <br>
  支持原生服务器 (Tokio) 和 Cloudflare Worker (WASM) 两种部署方式。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-Edition_2024-orange.svg" alt="Rust Version">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License">
</p>

<hr>

## 功能

- **群聊通知** — Issue 创建或更新时发一张按优先级配色的卡片到飞书群。连续更新会防抖合并，不刷屏。
- **指派私聊** — Issue 分配后通过邮箱匹配，自动给指派人发飞书私聊。
- **链接预览** — 在飞书粘贴 `linear.app` 链接，自动展开成摘要卡片（通过 Linear GraphQL API）。
- **签名校验** — Linear 用 HMAC-SHA256 验签，飞书用 token 校验。

## 路由

| Method | Path | 用途 |
| :--- | :--- | :--- |
| `POST` | `/webhook` | 接收 Linear Webhook |
| `POST` | `/lark/event` | 飞书事件回调 (Challenge 验证 + 链接预览) |
| `POST` | `/github/webhook` | 接收 GitHub Webhook |
| `GET`  | `/health` | 健康检查 |

## 快速开始

```bash
export LINEAR_WEBHOOK_SECRET=your_secret
export LARK_WEBHOOK_URL=https://open.larksuite.com/open-apis/bot/v2/hook/xxx
cargo run
```

完整环境变量说明见 [Configuration](./docs/configuration.md)。

## 部署

| 平台 | 文档 |
| :--- | :--- |
| Railway / Docker | [docs/deploy-railway.md](./docs/deploy-railway.md) |
| Cloudflare Workers | [docs/deploy-cloudflare-workers.md](./docs/deploy-cloudflare-workers.md) |

## 本地开发

1. 建一个飞书测试群，加自定义 Bot。在 Linear 新建 Webhook。
2. `ngrok http 3000` 拿到公网地址。
3. `cargo run`，把 ngrok 地址填进 Linear webhook。

## 许可证

[MIT](./LICENSE)
