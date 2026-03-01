# Deploy to Cloudflare Workers

LarkStack supports deploying as a Cloudflare Worker via the `cf-worker` feature flag.
The debounce logic runs on a Durable Object with alarms, so no persistent server is needed.

## Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [wrangler](https://developers.cloudflare.com/workers/wrangler/install-and-update/) CLI (`npm i -g wrangler`)
- Rust toolchain with `wasm32-unknown-unknown` target:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- [`worker-build`](https://crates.io/crates/worker-build):
  ```bash
  cargo install worker-build
  ```

## 1. Configure `wrangler.toml`

The repo includes a ready-to-use `wrangler.toml`. Edit the `[vars]` section to fill in
non-secret values:

```toml
[vars]
LARK_WEBHOOK_URL = "https://open.larksuite.com/open-apis/bot/v2/hook/xxx"
DEBOUNCE_DELAY_MS = "5000"
```

> `PORT` is ignored on Workers — Cloudflare handles routing.

## 2. Set Secrets

Secrets must **not** go in `wrangler.toml`. Use the CLI:

```bash
wrangler secret put LINEAR_WEBHOOK_SECRET
# paste your Linear webhook signing secret

# Optional — needed for DM-on-assign (Phase 2):
wrangler secret put LARK_APP_ID
wrangler secret put LARK_APP_SECRET

# Optional — needed for link previews (Phase 3):
wrangler secret put LINEAR_API_KEY
wrangler secret put LARK_VERIFICATION_TOKEN
```

## 3. Build & Deploy

```bash
wrangler deploy
```

On the first deploy, Wrangler runs the build command defined in `wrangler.toml`:

```
cargo install worker-build && worker-build --release
```

This compiles the crate with `--features cf-worker --target wasm32-unknown-unknown`
and generates the JS shim at `build/worker/shim.mjs`.

The `[[migrations]]` block in `wrangler.toml` automatically creates the
`DebounceObject` Durable Object class on first deploy.

## 4. Set Up Webhooks

After deploying, Wrangler prints your Worker URL (e.g. `https://larkstack.<your-subdomain>.workers.dev`).

| Service | URL to configure |
| :--- | :--- |
| Linear Webhook | `https://larkstack.xxx.workers.dev/webhook` |
| Lark Event Callback | `https://larkstack.xxx.workers.dev/lark/event` |

## Local Development

```bash
wrangler dev
```

This starts a local Workers runtime with Durable Object support.
Use `ngrok` to expose it if you need Linear / Lark to reach the local instance.

## How It Differs from Native (Railway / Docker)

| | Native (`cargo run`) | Cloudflare Worker |
| :--- | :--- | :--- |
| Runtime | Tokio multi-thread | V8 isolate (single-thread) |
| Debounce | In-memory `DebounceMap` + `tokio::spawn` | Durable Object + alarm |
| Config | Environment variables via `figment` | `wrangler.toml` vars + secrets |
| TLS | rustls | Handled by Cloudflare edge |
| Cold start | N/A (long-running) | ~50 ms (WASM) |

## Troubleshooting

**`DEBOUNCER binding not found`**
— The Durable Object binding is missing. Make sure `wrangler.toml` has:
```toml
[durable_objects]
bindings = [{ name = "DEBOUNCER", class_name = "DebounceObject" }]
```
And the `[[migrations]]` block is present for first deploy.

**`alarm: no event in storage`**
— The alarm fired but storage was empty. This can happen if a DO instance is
evicted and recreated. It's harmless — the log message is informational.

**Build fails with missing `wasm32-unknown-unknown`**
```bash
rustup target add wasm32-unknown-unknown
```
