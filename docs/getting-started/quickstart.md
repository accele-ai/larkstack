# Quick start

## Prerequisites

- Rust toolchain (edition 2024)
- A [Linear](https://linear.app/) workspace with webhook access
- A [Lark](https://larksuite.com/) group chat with a custom bot

## 1. Clone and run

```bash
git clone https://github.com/accele-ai/larkstack.git
cd larkstack
export LINEAR_WEBHOOK_SECRET=your_secret
export LARK_WEBHOOK_URL=https://open.larksuite.com/open-apis/bot/v2/hook/xxx
cargo run
```

The server starts on port 3000 by default.

## 2. Expose your local server

Use ngrok (or any tunnel) so Linear and Lark can reach you:

```bash
ngrok http 3000
```

## 3. Configure Linear

In your Linear workspace settings, create a webhook pointing at:

```
https://<YOUR_NGROK_URL>/webhook
```

Enable the event types you care about (Issue, Comment).

## 4. Test it

Create or update an issue in Linear. You should see a card appear in your Lark group
within a few seconds.

## What's next

- See [Configuration](configuration.md) for optional features (DM on assign, link previews).
- Ready to go live? Pick a deployment target: [Railway](../deploy/railway.md) or
  [Cloudflare Workers](../deploy/cloudflare-workers.md).
