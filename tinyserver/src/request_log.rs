// SPDX-License-Identifier: AGPL-3.0-or-later

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use tracing::info;

pub async fn log_request(
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    info!(
        client_ip = %client.ip(),
        method = %request.method(),
        path = request.uri().path(),
        "request received"
    );

    next.run(request).await
}
