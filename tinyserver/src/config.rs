// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    error::Error,
    fs, io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, DataChange, ModifyKind, RenameMode},
};
use serde::Deserialize;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub network: Network,
    #[serde(default)]
    pub library: Vec<Library>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Network {
    pub address: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub path: PathBuf,
    pub metadata_provider: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let contents = fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.network.address, self.network.port)
    }
}

pub fn path() -> Result<PathBuf, io::Error> {
    let base = dirs::config_local_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the operating system has no local configuration directory",
        )
    })?;

    Ok(base.join("tinystream").join("config.toml"))
}

pub fn watch(
    path: &Path,
) -> Result<(RecommendedWatcher, mpsc::UnboundedReceiver<()>), Box<dyn Error>> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path has no parent",
        )
    })?;
    let target = path.to_owned();
    let (sender, receiver) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        let Ok(event) = result else {
            tracing::warn!(error = %result.unwrap_err(), "configuration watcher error");
            return;
        };

        if event.paths.iter().any(|changed| changed == &target) && reload_event(&event.kind) {
            let _ = sender.send(());
        }
    })?;

    watcher.watch(directory, RecursiveMode::NonRecursive)?;
    Ok((watcher, receiver))
}

fn reload_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Any | CreateKind::File)
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Modify(ModifyKind::Data(DataChange::Any | DataChange::Content))
            | EventKind::Modify(ModifyKind::Name(
                RenameMode::Any | RenameMode::To | RenameMode::Both
            ))
    )
}
