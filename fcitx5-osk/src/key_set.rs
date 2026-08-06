use std::{
    collections::HashMap,
    fmt::{Display, Formatter as FmtFormatter, Result as FmtResult},
    mem::MaybeUninit,
    path::PathBuf,
    rc::Rc,
    result::Result as StdResult,
    sync::{Arc, LazyLock, RwLock},
};

use getset::Getters;
use iced::Font;
use serde::{
    Deserialize, Deserializer,
    de::{Error, Unexpected},
};
use xkeysym::Keysym;

use crate::{
    font::{self, DEFAULT_NERD_FONT_ID},
    store::IdAndConfigPath,
};

static KEY_VALUE_POOL: LazyLock<Arc<RwLock<HashMap<char, KeyValue>>>> =
    LazyLock::new(Default::default);

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

#[derive(Debug, PartialEq, Eq)]
struct KeyValueInner {
    symbol: String,
    keysym: Keysym,
    keycode: Option<i16>,
    font: Option<Font>,
}

#[derive(Debug)]
pub struct KeyValue {
    inner: MaybeUninit<Arc<KeyValueInner>>,
}

impl KeyValue {
    fn inner(&self) -> &Arc<KeyValueInner> {
        unsafe {
            // # Safety, it will be always valid before drop
            self.inner.assume_init_ref()
        }
    }

    pub fn symbol(&self) -> &str {
        &self.inner().symbol
    }

    pub fn keysym(&self) -> Keysym {
        self.inner().keysym
    }

    pub fn keycode(&self) -> Option<i16> {
        self.inner().keycode
    }

    pub fn font(&self) -> Option<Font> {
        self.inner().font
    }

    pub fn from_char(c: char) -> Option<Self> {
        match KEY_VALUE_POOL.read() {
            Ok(p) => {
                let kv = p.get(&c);
                if kv.is_some() {
                    return kv.cloned();
                }
            }
            Err(_) => tracing::warn!("KEY_VALUE_POOL is poisoned, try creating a new KeyValue"),
        }

        let (kc, s) = match c {
            '1' => (10, "1"),
            '2' => (11, "2"),
            '3' => (12, "3"),
            '4' => (13, "4"),
            '5' => (14, "5"),
            '6' => (15, "6"),
            '7' => (16, "7"),
            '8' => (17, "8"),
            '9' => (18, "9"),
            '0' => (19, "0"),
            'q' => (24, "q"),
            'w' => (25, "w"),
            'e' => (26, "e"),
            'r' => (27, "r"),
            't' => (28, "t"),
            'y' => (29, "y"),
            'u' => (30, "u"),
            'i' => (31, "i"),
            'o' => (32, "o"),
            'p' => (33, "p"),
            'a' => (38, "a"),
            's' => (39, "s"),
            'd' => (40, "d"),
            'f' => (41, "f"),
            'g' => (42, "g"),
            'h' => (43, "h"),
            'j' => (44, "j"),
            'k' => (45, "k"),
            'l' => (46, "l"),
            'z' => (52, "z"),
            'x' => (53, "x"),
            'c' => (54, "c"),
            'v' => (55, "v"),
            'b' => (56, "b"),
            'n' => (57, "n"),
            'm' => (58, "m"),
            'Q' => (-24, "Q"),
            'W' => (-25, "W"),
            'E' => (-26, "E"),
            'R' => (-27, "R"),
            'T' => (-28, "T"),
            'Y' => (-29, "Y"),
            'U' => (-30, "U"),
            'I' => (-31, "I"),
            'O' => (-32, "O"),
            'P' => (-33, "P"),
            'A' => (-38, "A"),
            'S' => (-39, "S"),
            'D' => (-40, "D"),
            'F' => (-41, "F"),
            'G' => (-42, "G"),
            'H' => (-43, "H"),
            'J' => (-44, "J"),
            'K' => (-45, "K"),
            'L' => (-46, "L"),
            'Z' => (-52, "Z"),
            'X' => (-53, "X"),
            'C' => (-54, "C"),
            'V' => (-55, "V"),
            'B' => (-56, "B"),
            'N' => (-57, "N"),
            'M' => (-58, "M"),
            ' ' => (65, " "),
            '!' => (-10, "!"),
            '@' => (-11, "@"),
            '#' => (-12, "#"),
            '$' => (-13, "$"),
            '%' => (-14, "%"),
            '^' => (-15, "^"),
            '&' => (-16, "&"),
            '*' => (-17, "*"),
            '(' => (-18, "("),
            ')' => (-19, ")"),
            '-' => (20, "-"),
            '_' => (-20, "_"),
            '=' => (21, "="),
            '+' => (-21, "+"),
            '[' => (34, "["),
            '{' => (-34, "{"),
            ']' => (35, "]"),
            '}' => (-35, "}"),
            '\\' => (51, "\\"),
            '|' => (-51, "|"),
            ';' => (47, ";"),
            ':' => (-47, ":"),
            '\'' => (48, "'"),
            '"' => (-48, "\""),
            ',' => (59, ","),
            '<' => (-59, "<"),
            '.' => (60, "."),
            '>' => (-60, ">"),
            '/' => (61, "/"),
            '?' => (-61, "?"),
            '`' => (49, "`"),
            '~' => (-49, "~"),
            '\n' => (36, "󰌑"),
            _ => {
                tracing::warn!("Unsupported char[{c}]");
                return None;
            }
        };

        let font = if c == '\n' {
            Some(font::load(DEFAULT_NERD_FONT_ID))
        } else {
            None
        };

        let kv = Self {
            inner: MaybeUninit::new(Arc::new(KeyValueInner {
                symbol: s.to_string(),
                keysym: Keysym::from_char(c),
                keycode: Some(kc),
                font,
            })),
        };

        let _ = KEY_VALUE_POOL.write().map(|mut p| p.insert(c, kv.clone()));

        Some(kv)
    }
}

impl Drop for KeyValue {
    fn drop(&mut self) {
        unsafe {
            // # Safety, it will be always valid before drop
            self.inner.assume_init_drop();
        }
        // zeroize
        self.inner = MaybeUninit::zeroed();
    }
}

impl Clone for KeyValue {
    fn clone(&self) -> Self {
        Self {
            inner: MaybeUninit::new(self.inner().clone()),
        }
    }
}

impl PartialEq for KeyValue {
    fn eq(&self, other: &Self) -> bool {
        self.inner() == other.inner()
    }
}

impl Eq for KeyValue {}

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
            inner: MaybeUninit::new(Arc::new(KeyValueInner {
                symbol,
                keysym,
                keycode: raw.keycode,
                font: raw.font.as_deref().map(font::load),
            })),
        })
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

    pub fn rotate(&self, cur: usize) -> RotatedKey<'_> {
        let cur = cur % (self.raw.secondaries.len() + 1);
        let (primary, secondaries) = if cur == 0 {
            (&self.raw.primary, self.raw.secondaries.iter().collect())
        } else {
            let mut secondaries = vec![];
            secondaries.extend(self.raw.secondaries[cur..].iter());
            secondaries.push(&self.raw.primary);
            secondaries.extend(self.raw.secondaries[..cur - 1].iter());
            (&self.raw.secondaries[cur - 1], secondaries)
        };
        RotatedKey {
            primary,
            secondaries,
        }
    }

    pub fn len(&self) -> usize {
        self.raw.secondaries.len() + 1
    }
}

pub struct RotatedKey<'a> {
    primary: &'a KeyValue,
    secondaries: Vec<&'a KeyValue>,
}

impl<'a> RotatedKey<'a> {
    pub fn key_value(&self, shift: bool, caps_lock: bool) -> KeyValue {
        let key_value = if Key::is_shifted(shift, caps_lock) {
            self.secondaries.first().copied().unwrap_or(self.primary)
        } else {
            self.primary
        };
        key_value.clone()
    }

    pub fn primary(&self) -> &'a KeyValue {
        self.primary
    }

    pub fn secondaries(&self) -> &[&'a KeyValue] {
        &self.secondaries
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

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ComboKey {
    Key(KeyValue),
    Release,
    ReleaseAll,
}

impl Display for ComboKey {
    fn fmt(&self, f: &mut FmtFormatter<'_>) -> FmtResult {
        let s = match self {
            ComboKey::Key(key_value) => key_value.symbol(),
            ComboKey::Release => "Release",
            ComboKey::ReleaseAll => "ReleaseAll",
        };
        f.write_str(s)
    }
}

impl ComboKey {
    pub fn font(&self) -> Option<Font> {
        match self {
            ComboKey::Key(key_value) => key_value.font(),
            ComboKey::Release => None,
            ComboKey::ReleaseAll => None,
        }
    }
}

#[derive(Deserialize, Getters)]
pub struct ComboKeyGroup {
    #[getset(get = "pub")]
    pub keys: Vec<ComboKey>,
}
