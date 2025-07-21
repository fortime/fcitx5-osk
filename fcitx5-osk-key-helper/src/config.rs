use std::path::PathBuf;

use anyhow::Result;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, CopyGetters, Getters, Clone)]
pub struct Config {
    #[getset(get_copy = "pub")]
    log_timestamp: Option<bool>,

    #[getset(get = "pub")]
    #[serde(default)]
    log_directives: Vec<String>,

    /// These keycodes are x11 variant, they are +8 shift of evdev keycodes
    #[getset(get = "pub")]
    #[serde(default = "default_keycodes")]
    keycodes: Vec<u16>,
}

impl Config {
    pub fn new(path: &PathBuf) -> Result<Self> {
        let config = if path.exists() {
            Figment::new().merge(Toml::file(path)).extract()?
        } else {
            Figment::new().extract()?
        };
        Ok(config)
    }
}

fn default_keycodes() -> Vec<u16> {
    vec![
        37,  // Left Ctrl
        105, // Right Ctrl
        50,  // Left Shift
        62,  // Right Shift
        64,  // Left Alt
        108, // Right Alt
    ]
}
