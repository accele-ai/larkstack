//! Linear webhook source — receives issue/comment events and converts them
//! to the unified [`Event`](crate::event::Event) model.

pub mod client;
mod handler;
pub mod models;
pub mod utils;

pub use handler::webhook_handler;
