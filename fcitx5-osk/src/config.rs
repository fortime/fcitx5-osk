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
use strum::EnumIter;
use tokio::fs;

#[derive(Deserialize, Serialize, CopyGetters, Getters, Setters, Clone)]
pub struct Config {
    #[getset(get_copy = "pub")]
    log_timestamp: Option<bool>,

    #[getset(get = "pub")]
    #[serde(default)]
    log_directives: Vec<String>,

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

    #[serde(default)]
    dark_theme: Option<String>,

    #[serde(default)]
    light_theme: Option<String>,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default)]
    placement: Placement,

    /// Default font to be used.
    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    default_font: Option<String>,

    /// Load fonts by path.
    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    external_font_paths: Vec<PathBuf>,

    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    im_layout_mapping: HashMap<String, HashMap<String, String>>,

    #[getset(get = "pub", set = "pub")]
    #[serde(default)]
    im_font_mapping: HashMap<String, String>,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default = "default_indicator_width")]
    indicator_width: u16,

    #[getset(get_copy = "pub", set = "pub")]
    #[serde(default)]
    indicator_display: IndicatorDisplay,

    /// Preferred output to be used.
    #[serde(default)]
    preferred_output_name: Option<String>,

    /// These keycodes are x11 variant, they are +8 shift of evdev keycodes.
    #[getset(get = "pub", set = "pub")]
    #[serde(default = "default_modifier_workaround_keycodes")]
    modifier_workaround_keycodes: Vec<u16>,

    /// How long will the keyboard wait after receiving a signal from fcitx5.
    #[getset(get = "pub", set = "pub")]
    #[serde(with = "humantime_serde", default = "default_hide_delay")]
    hide_delay: Duration,
}

impl Config {
    pub fn dark_theme(&self) -> Option<&String> {
        self.dark_theme.as_ref()
    }

    pub fn set_dark_theme(&mut self, theme: String) {
        self.dark_theme = Some(theme);
    }

    pub fn light_theme(&self) -> Option<&String> {
        self.light_theme.as_ref()
    }

    pub fn set_light_theme(&mut self, theme: String) {
        self.light_theme = Some(theme);
    }

    pub fn preferred_output_name(&self) -> Option<&String> {
        self.preferred_output_name.as_ref()
    }

    pub fn set_preferred_output_name(&mut self, name: String) {
        self.preferred_output_name = Some(name);
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
    Duration::from_millis(200)
}

fn default_hide_delay() -> Duration {
    Duration::from_millis(1000)
}

fn default_theme() -> String {
    "Auto".to_string()
}

fn default_modifier_workaround_keycodes() -> Vec<u16> {
    vec![
        37,  // Left Ctrl
        105, // Right Ctrl
        50,  // Left Shift
        62,  // Right Shift
        64,  // Left Alt
        108, // Right Alt
    ]
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
            Figment::new().extract()?
        };
        let (tx, mut rx) = mpsc::unbounded();
        let res = Self {
            _path: path.clone(),
            config,
            writer: tx,
        };
        let path = path.clone();
        let bg = async move {
            while let Some(mut latest) = rx.next().await {
                let closed = loop {
                    match rx.try_next() {
                        Ok(Some(c)) => latest = c,
                        // closed
                        Ok(None) => break true,
                        // no message
                        Err(_) => break false,
                    }
                };

                if let Some(parent) = path.parent() {
                    if !parent.exists() {
                        if let Err(e) = fs::create_dir_all(parent).await {
                            tracing::error!("writing {parent:?} failed: {e}");
                        }
                    }
                }

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
        if self.writer.unbounded_send(content).is_err() {
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

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, EnumIter, strum::Display,
)]
pub enum Placement {
    #[default]
    Dock,
    Float,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, EnumIter, strum::Display,
)]
pub enum IndicatorDisplay {
    #[default]
    Auto,
    AlwaysOn,
    AlwaysOff,
}
