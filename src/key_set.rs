use std::{collections::HashMap, path::PathBuf, rc::Rc, result::Result as StdResult};

use getset::Getters;
use serde::{de::Error, Deserialize, Deserializer};
use xkeysym::Keysym;

use crate::store::IdAndConfigPath;

#[derive(Deserialize)]
struct RawKeyValue {
    #[serde(alias = "s")]
    symbol: Option<String>,
    #[serde(alias = "ks")]
    keysym: Option<u32>,
    #[serde(alias = "c")]
    character: Option<char>,
}

#[derive(Getters)]
pub struct KeyValue {
    #[getset(get = "pub")]
    symbol: String,
    #[getset(get = "pub")]
    keysym: Keysym,
}

impl<'de> Deserialize<'de> for KeyValue {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: RawKeyValue = Deserialize::deserialize(deserializer)?;
        let keysym = if let Some(ks) = raw.keysym {
            Keysym::from(ks)
        } else {
            if let Some(c) = raw.character {
                Keysym::from_char(c)
            } else {
                return Err(Error::missing_field(&"ks or c"));
            }
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
        tracing::debug!("symbol of {:x}: {}", u32::from(keysym), symbol);
        Ok(Self { symbol, keysym })
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
        Ok(Self {
            raw: Rc::new(raw),
        })
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
