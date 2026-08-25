// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{error::Error, future::IntoFuture, net::SocketAddr};

use axum::Router;
use tokio::{net::TcpListener, signal, sync::oneshot};
use tracing::{error, info};

use crate::config::{self, Config};

pub async fn run(app: Router) -> Result<(), Box<dyn Error>> {
    let config_path = config::path()?;
    let (_watcher, mut changes) = config::watch(&config_path)?;
    let mut current = Config::load(&config_path)?;
    let mut listener = TcpListener::bind(current.socket_addr()).await?;
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        info!(address = %listener.local_addr()?, config = %config_path.display(), "tinyserver listening");

        let (stop_sender, stop_receiver) = oneshot::channel();
        let mut stop_sender = Some(stop_sender);
        let service = app
            .clone()
            .into_make_service_with_connect_info::<SocketAddr>();
        let server = axum::serve(listener, service)
            .with_graceful_shutdown(async {
                let _ = stop_receiver.await;
            })
            .into_future();
        tokio::pin!(server);

        loop {
            tokio::select! {
                result = &mut server => return Ok(result?),
                () = &mut shutdown => {
                    info!("shutdown signal received");
                    let _ = stop_sender.take().expect("shutdown sender is available").send(());
                    server.await?;
                    return Ok(());
                }
                change = changes.recv() => {
                    let Some(()) = change else {
                        return Err("configuration watcher stopped unexpectedly".into());
                    };

                    let updated = match Config::load(&config_path) {
                        Ok(updated) => updated,
                        Err(error) => {
                            error!(%error, "failed to reload configuration; retaining current values");
                            continue;
                        }
                    };

                    if updated == current {
                        continue;
                    }

                    let replacement = match TcpListener::bind(updated.socket_addr()).await {
                        Ok(listener) => listener,
                        Err(error) => {
                            error!(address = %updated.socket_addr(), %error, "failed to apply configuration; retaining current listener");
                            continue;
                        }
                    };

                    info!(old_address = %current.socket_addr(), new_address = %updated.socket_addr(), "configuration reloaded");
                    let _ = stop_sender.take().expect("shutdown sender is available").send(());
                    server.await?;
                    current = updated;
                    listener = replacement;
                    break;
                }
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
