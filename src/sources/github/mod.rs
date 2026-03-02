//! GitHub webhook source — receives PR, push, and issue events and converts
//! them to the unified [`Event`](crate::event::Event) model.

mod handler;
mod utils;

pub use handler::webhook_handler;
