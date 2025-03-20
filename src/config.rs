use std::{collections::HashMap, future::Future, path::PathBuf, time::Duration};

use anyhow::Result;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use getset::{CopyGetters, Getters, Setters};
use iced::futures::{
    channel::mpsc::{self, UnboundedSender},
    StreamExt,
};
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
    #[serde(default = "default_landscape_width")]
    landscape_width: u16,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default = "default_portrait_width")]
    portrait_width: u16,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(with = "humantime_serde", default = "default_holding_timeout")]
    holding_timeout: Duration,

    #[getset(get = "pub", set = "pub")]
    #[serde(default = "default_theme")]
    theme: String,

    dark_theme: Option<String>,

    light_theme: Option<String>,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default)]
    placement: Placement,

    /// default font to be used.
    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    default_font: Option<String>,

    /// load fonts by path
    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    external_font_paths: Vec<PathBuf>,

    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    im_layout_mapping: HashMap<String, String>,

    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    im_font_mapping: HashMap<String, String>,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default = "default_indicator_width")]
    indicator_width: u16,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default)]
    indicator_display: IndicatorDisplay,
}

impl Config {
    pub fn dark_theme(&self) -> Option<&str> {
        self.dark_theme.as_deref()
    }

    pub fn light_theme(&self) -> Option<&str> {
        self.light_theme.as_deref()
    }
}

fn default_landscape_width() -> u16 {
    1024
}

fn default_portrait_width() -> u16 {
    768
}

fn default_indicator_width() -> u16 {
    80
}

fn default_holding_timeout() -> Duration {
    Duration::from_millis(500)
}

fn default_theme() -> String {
    "Auto".to_string()
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

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Dock,
    Float,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum IndicatorDisplay {
    #[default]
    Auto,
    AlwaysOn,
    AlwaysOff,
}
