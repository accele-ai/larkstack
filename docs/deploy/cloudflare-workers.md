# Deploy to Cloudflare Workers

LarkStack supports deploying as a Cloudflare Worker via the `cf-worker` feature flag.
Debounce runs on a Durable Object with alarms, so no persistent server is needed.

## Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [wrangler](https://developers.cloudflare.com/workers/wrangler/install-and-update/) CLI
- Rust toolchain with `wasm32-unknown-unknown` target:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- [`worker-build`](https://crates.io/crates/worker-build):
  ```bash
  cargo install worker-build
  ```

## 1. Configure `wrangler.toml`

Edit the `[vars]` section:

```toml
[vars]
LARK_WEBHOOK_URL = "https://open.larksuite.com/open-apis/bot/v2/hook/xxx"
DEBOUNCE_DELAY_MS = "5000"
```

`PORT` is ignored on Workers.

## 2. Set secrets

Secrets must not go in `wrangler.toml`. Use the CLI:

```bash
wrangler secret put LINEAR_WEBHOOK_SECRET
# paste your Linear webhook signing secret

# Optional — DM on assign:
wrangler secret put LARK_APP_ID
wrangler secret put LARK_APP_SECRET

# Optional — link previews:
wrangler secret put LINEAR_API_KEY
wrangler secret put LARK_VERIFICATION_TOKEN
```

## 3. Build and deploy

```bash
wrangler deploy
```

On the first deploy, Wrangler runs:

```
cargo install worker-build && worker-build --release
```

This compiles with `--features cf-worker --target wasm32-unknown-unknown` and
generates the JS shim at `build/worker/shim.mjs`. The `[[migrations]]` block in
`wrangler.toml` creates the `DebounceObject` Durable Object class automatically.

## 4. Set up webhooks

After deploying, Wrangler prints your Worker URL.

| Service | URL |
| :--- | :--- |
| Linear Webhook | `https://larkstack.xxx.workers.dev/webhook` |
| Lark Event Callback | `https://larkstack.xxx.workers.dev/lark/event` |

## Local development

```bash
wrangler dev
```

Starts a local Workers runtime with Durable Object support. Use ngrok if you need
Linear / Lark to reach the local instance.

## Native vs. Worker differences

| | Native (`cargo run`) | Cloudflare Worker |
| :--- | :--- | :--- |
| Runtime | Tokio multi-thread | V8 isolate (single-thread) |
| Debounce | In-memory `DebounceMap` + `tokio::spawn` | Durable Object + alarm |
| Config | Environment variables via `figment` | `wrangler.toml` vars + secrets |
| TLS | rustls | Cloudflare edge |
| Cold start | N/A (long-running) | ~50 ms (WASM) |

## Troubleshooting

**`DEBOUNCER binding not found`** — Make sure `wrangler.toml` has:

```toml
[durable_objects]
bindings = [{ name = "DEBOUNCER", class_name = "DebounceObject" }]
```

and the `[[migrations]]` block is present.

**`alarm: no event in storage`** — The alarm fired but storage was empty. This
happens if a DO instance is evicted and recreated. It's harmless.

**Build fails with missing `wasm32-unknown-unknown`**:

```bash
rustup target add wasm32-unknown-unknown
```
