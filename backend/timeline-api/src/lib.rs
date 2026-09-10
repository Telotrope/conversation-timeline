//! The deployed backend's axum app: route handlers behind API Gateway, per
//! V2 of the migration plan. `app::build_router` is the single place the
//! whole app is wired together; `main.rs` just decides whether to run it
//! locally or inside Lambda.

pub mod app;
pub mod auth_extractor;
pub mod error;
pub mod routes;
pub mod state;
