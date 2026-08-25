// SPDX-License-Identifier: AGPL-3.0-or-later

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub database: SqlitePool,
}

pub fn router(database: SqlitePool) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .with_state(AppState { database });

    #[cfg(debug_assertions)]
    let router = router.layer(axum::middleware::from_fn(crate::request_log::log_request));

    router
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health(
    State(AppState {
        database: _database,
    }): State<AppState>,
) -> Json<Health> {
    Json(Health { status: "ok" })
}
