#[cfg(all(feature = "native", feature = "cf-worker"))]
compile_error!("features `native` and `cf-worker` are mutually exclusive");

pub mod config;
pub mod dispatch;
pub mod event;
pub mod sinks;
pub mod sources;
pub mod utils;

#[cfg(not(feature = "cf-worker"))]
pub mod debounce;

#[cfg(feature = "cf-worker")]
pub mod debounce_do;

#[cfg(feature = "cf-worker")]
mod cf_entry {
    use std::sync::Arc;

    use axum::http::StatusCode;
    use worker::*;

    use crate::config::AppState;

    #[event(fetch)]
    async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<HttpResponse> {
        let state = Arc::new(AppState::from_worker_env(env));

        let (parts, body) = req.into_parts();
        let body_bytes = read_body(body).await?;

        match (parts.method.as_str(), parts.uri.path()) {
            ("POST", "/webhook") => {
                let status = crate::sources::linear::webhook_handler(
                    axum::extract::State(state),
                    parts.headers,
                    body_bytes,
                )
                .await;
                text_response(status, "")
            }
            ("POST", "/github/webhook") => {
                let status = crate::sources::github::webhook_handler(
                    axum::extract::State(state),
                    parts.headers,
                    body_bytes,
                )
                .await;
                text_response(status, "")
            }
            ("POST", "/lark/event") => {
                let (status, axum::Json(json)) =
                    crate::sinks::lark::lark_event_handler(axum::extract::State(state), body_bytes)
                        .await;
                json_response(status, &json)
            }
            ("GET", "/health") => text_response(StatusCode::OK, "ok"),
            _ => text_response(StatusCode::NOT_FOUND, "not found"),
        }
    }

    async fn read_body(body: Body) -> Result<axum::body::Bytes> {
        use http_body_util::BodyExt;
        body.collect()
            .await
            .map(|c| c.to_bytes())
            .map_err(|e| Error::RustError(format!("read body: {e}")))
    }

    fn text_response(status: StatusCode, text: &str) -> Result<HttpResponse> {
        Response::ok(text)?.with_status(status.as_u16()).try_into()
    }

    fn json_response(status: StatusCode, json: &serde_json::Value) -> Result<HttpResponse> {
        Response::from_json(json)?
            .with_status(status.as_u16())
            .try_into()
    }
}
