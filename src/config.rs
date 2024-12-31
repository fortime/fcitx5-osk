use std::{future::Future, path::PathBuf};

use anyhow::Result;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use getset::{CopyGetters, Getters, Setters};
use iced::futures::{channel::mpsc::{self, UnboundedSender}, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Deserialize, Serialize, CopyGetters, Getters, Setters, Default, Clone)]
pub struct Config {
    #[getset(get_copy = "pub")]
    log_timestamp: Option<bool>,

    #[getset(get = "pub")]
    #[serde(default)]
    key_area_layout_folders: Vec<PathBuf>,

    #[getset(get = "pub")]
    #[serde(default)]
    key_set_folders: Vec<PathBuf>,

    #[getset(get_copy = "pub", set = "pub")]
    width: u16,

    #[getset(get = "pub", set = "pub")]
    theme: String,
}

pub struct ConfigManager {
    _path: PathBuf,
    config: Config,
    writer: UnboundedSender<String>,
}

impl ConfigManager {
    pub fn new(path: &PathBuf) -> Result<(Self, impl Future<Output = ()> + 'static + Send + Sync)> {
        let config = if path.exists() {
            Figment::new().merge(Toml::file(path)).extract()?
        } else {
            Default::default()
        };
        let (tx, mut rx) = mpsc::unbounded();
        let res = Self {
            _path: path.clone(),
            config,
            writer: tx,
        };
        let path = path.clone();
        let bg = async move {
            loop {
                let mut latest = if let Some(c) = rx.next().await {
                    c
                } else {
                    break;
                };
                let closed = loop {
                    match rx.try_next() {
                        Ok(Some(c)) => latest = c,
                        // closed
                        Ok(None) => break true,
                        // no message
                        Err(_) => break false,
                    }
                };

                if let Err(e) = fs::write(&path, latest).await {
                    tracing::error!("writing {path:?} failed: {e}");
                }

                if closed {
                    break;
                }
            }
            tracing::info!("config writing bg exits");
        };
        Ok((res, bg))
    }

    pub fn try_write(&mut self) -> bool {
        let content = match toml::to_string(&self.config) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to serialize config: {e}");
                return false;
            }
        };
        if let Err(_) = self.writer.unbounded_send(content) {
            tracing::warn!("failed to write config, channel is closed");
            false
        } else {
            true
        }
    }
}

impl AsRef<Config> for ConfigManager {
    fn as_ref(&self) -> &Config {
        &self.config
    }
}

impl AsMut<Config> for ConfigManager {
    fn as_mut(&mut self) -> &mut Config {
        &mut self.config
    }
}
