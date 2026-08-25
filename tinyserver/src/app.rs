// SPDX-License-Identifier: AGPL-3.0-or-later

use axum::{Json, Router, routing::get};
use serde::Serialize;

pub fn router() -> Router {
    let router = Router::new().route("/health", get(health));

    #[cfg(debug_assertions)]
    let router = router.layer(axum::middleware::from_fn(crate::request_log::log_request));

    router
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}
