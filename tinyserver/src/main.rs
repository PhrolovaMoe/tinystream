// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Copyright (C) 2026 Phrolova <me@phrolova.moe>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public
// License along with this program. If not, see
// <https://www.gnu.org/licenses/>.

mod app;
mod config;
mod database;
mod library;
#[cfg(debug_assertions)]
mod request_log;
mod server;

use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let database = database::connect().await?;
    let state = app::AppState { database };
    server::run(app::router(state.clone()), state).await
}
