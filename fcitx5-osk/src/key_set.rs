use std::{collections::HashMap, path::PathBuf, rc::Rc, result::Result as StdResult};

use getset::{CopyGetters, Getters};
use iced::Font;
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};
use xkeysym::Keysym;

use crate::{font, store::IdAndConfigPath};

#[derive(Deserialize)]
struct RawKeyValue {
    #[serde(alias = "s")]
    symbol: Option<String>,
    #[serde(alias = "ks")]
    keysym: Option<u32>,
    #[serde(alias = "c")]
    character: Option<char>,
    #[serde(alias = "kc")]
    keycode: Option<i16>,
    #[serde(alias = "f")]
    font: Option<String>,
}

#[derive(CopyGetters, Getters)]
pub struct KeyValue {
    #[getset(get = "pub")]
    symbol: String,
    #[getset(get_copy = "pub")]
    keysym: Keysym,
    #[getset(get_copy = "pub")]
    keycode: Option<i16>,
    #[getset(get_copy = "pub")]
    font: Option<Font>,
}

#[derive(CopyGetters, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThinKeyValue {
    #[getset(get_copy = "pub")]
    keysym: Keysym,
    #[getset(get_copy = "pub")]
    keycode: Option<i16>,
}

impl<'de> Deserialize<'de> for KeyValue {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: RawKeyValue = Deserialize::deserialize(deserializer)?;
        let keysym = if let Some(ks) = raw.keysym {
            Keysym::from(ks)
        } else if let Some(c) = raw.character {
            Keysym::from_char(c)
        } else {
            return Err(Error::missing_field("ks or c"));
        };
        let symbol = if let Some(symbol) = raw.symbol {
            symbol
        } else {
            match keysym.key_char() {
                Some(c) if !c.is_control() && !c.is_whitespace() => c.to_string(),
                _ => keysym
                    .name()
                    .and_then(|n| n.splitn(2, "_").last())
                    .unwrap_or("Unknown")
                    .to_string(),
            }
        };
        if let Some(keycode) = raw.keycode {
            // check the abs of keycode is smaller than 256.
            if keycode.abs() >= u8::MAX as i16 || keycode.abs() < 8 {
                return Err(Error::invalid_value(
                    Unexpected::Signed(keycode as i64),
                    &"8<= kc < 256",
                ));
            }
        }
        tracing::debug!("symbol of {:x}: {}", u32::from(keysym), symbol);
        Ok(Self {
            symbol,
            keysym,
            keycode: raw.keycode,
            font: raw.font.as_deref().map(font::load),
        })
    }
}

impl KeyValue {
    pub fn to_thin(&self) -> ThinKeyValue {
        ThinKeyValue {
            keysym: self.keysym,
            keycode: self.keycode,
        }
    }
}

#[derive(Deserialize)]
struct RawKey {
    #[serde(alias = "p")]
    primary: KeyValue,
    #[serde(default, alias = "s")]
    secondaries: Vec<KeyValue>,
}

#[derive(Clone)]
pub struct Key {
    raw: Rc<RawKey>,
}

impl Key {
    pub fn is_shifted(shift: bool, caps_lock: bool) -> bool {
        shift ^ caps_lock
    }

    pub fn key_value(&self, shift: bool, caps_lock: bool) -> ThinKeyValue {
        let key_value = if Self::is_shifted(shift, caps_lock) {
            self.raw.secondaries.first().unwrap_or(&self.raw.primary)
        } else {
            &self.raw.primary
        };
        key_value.to_thin()
    }

    pub fn has_secondary(&self) -> bool {
        !self.raw.secondaries.is_empty()
    }

    pub fn primary(&self) -> &KeyValue {
        &self.raw.primary
    }

    pub fn secondaries(&self) -> &[KeyValue] {
        &self.raw.secondaries
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: RawKey = Deserialize::deserialize(deserializer)?;
        Ok(Self { raw: Rc::new(raw) })
    }
}

#[derive(Deserialize, Getters)]
pub struct KeySet {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    keys: HashMap<String, Key>,
}

impl IdAndConfigPath for KeySet {
    type IdType = String;

    fn id(&self) -> &Self::IdType {
        &self.name
    }

    fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    fn set_path<T: Into<PathBuf>>(&mut self, path: T) {
        self.path = Some(path.into());
    }
}
