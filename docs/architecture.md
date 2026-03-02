# Architecture

LarkStack is a three-layer pipeline. Sources receive webhooks and normalize them into
a unified `Event`, sinks format and deliver notifications, and the middle layer
(debounce + dispatch) operates only on `Event`. Adding a new source or sink requires
no changes to the other layers.

## Source code layout

```
src/
├── sources/
│   └── linear/          # Linear webhook source
│       ├── handler.rs   # POST /webhook — signature check, parse, normalize
│       ├── models.rs    # Linear JSON payload types
│       ├── client.rs    # GraphQL client (link previews)
│       └── utils.rs
├── sinks/
│   └── lark/            # Lark notification sink
│       ├── cards.rs     # Card builders (pure functions)
│       ├── bot.rs       # Tenant token cache + HTTP client
│       ├── webhook.rs   # Group webhook sender
│       ├── event_handler.rs  # POST /lark/event — challenge + link preview
│       └── models.rs
├── event.rs             # Unified Event enum (IssueCreated, IssueUpdated, CommentCreated)
├── dispatch.rs          # Routes an Event to all sinks
├── debounce.rs          # Native: in-memory DebounceMap + tokio timers
├── debounce_do.rs       # CF Workers: Durable Object + alarm
├── config.rs            # AppState, env var parsing (figment / worker::Env)
├── main.rs              # Native entrypoint (axum + tokio)
└── lib.rs               # Feature-gated CF Worker entrypoint
```

## Data flow

A Linear webhook arrives at `POST /webhook`. The handler verifies the HMAC signature,
deserializes the payload, and converts it into an `Event`. The event goes through the
debounce layer (which coalesces rapid updates on the same issue within a configurable
window), then dispatch sends it to every registered sink.

Currently there is one source (Linear) and one sink (Lark), but the pipeline is
designed so either side can be extended independently.

## Feature flags

The crate has two mutually exclusive feature flags:

| Flag | Runtime | Debounce strategy |
| :--- | :--- | :--- |
| `native` (default) | Tokio multi-thread | In-memory `DebounceMap` + `tokio::spawn` |
| `cf-worker` | V8 isolate (WASM) | Durable Object + alarm API |

Both share the same source/sink/event code. Only the entrypoint and debounce
implementation differ.

## Dependencies

- `axum 0.8` — HTTP routing (used in both native and Worker builds)
- `tokio` — async runtime (full features in native, sync-only in Worker)
- `reqwest 0.12` — HTTP client for outbound Lark / Linear API calls
- `figment` — config from env vars (native only)
- `worker` — Cloudflare Workers bindings (Worker only)
- `hmac` + `sha2` — webhook signature verification
