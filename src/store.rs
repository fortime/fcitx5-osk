use core::hash::Hash;
use std::{collections::HashMap, fmt::Display, path::PathBuf, rc::Rc};

use crate::{
    config::Config,
    font,
    key_set::{Key, KeySet, KeyValue},
    layout::{KeyAreaLayout, KeyId},
};

use anyhow::Result;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use iced::{Font, Theme};
use serde::Deserialize;

mod default_value;

pub(crate) trait IdAndConfigPath {
    type IdType;

    fn id(&self) -> &Self::IdType;

    fn path(&self) -> Option<&PathBuf>;

    fn set_path<T: Into<PathBuf>>(&mut self, path: T);
}

pub struct Store {
    theme_names: Vec<String>,
    themes: HashMap<String, Theme>,
    default_key_area_layout: Rc<KeyAreaLayout>,
    key_area_layouts: HashMap<String, Rc<KeyAreaLayout>>,
    default_key_set: Rc<KeySet>,
    key_sets: HashMap<String, Rc<KeySet>>,
    im_layout_mapping: HashMap<String, String>,
    im_font_mapping: HashMap<String, Font>,
}

impl Store {
    pub fn new(config: &Config) -> Result<Self> {
        let themes = Theme::ALL
            .iter()
            .map(|t| (t.to_string().to_lowercase(), t.clone()))
            .collect();
        let mut theme_names = Vec::with_capacity(Theme::ALL.len() + 1);
        theme_names.push("Auto".to_string());
        Theme::ALL
            .iter()
            .for_each(|t| theme_names.push(t.to_string()));
        let default_key_area_layout =
            Rc::new(init_default(default_value::DEFAULT_KEY_AREA_LAYOUT_TOML)?);
        let key_area_layouts = init_confs(&config.key_area_layout_folders())?;
        let default_key_set = Rc::new(init_default(default_value::DEFAULT_KEY_SET_TOML)?);
        let key_sets = init_confs(&config.key_set_folders())?;
        let im_layout_mapping = config.im_layout_mapping().clone();
        let im_font_mapping = config
            .im_font_mapping()
            .iter()
            .map(|(k, v)| (k.clone(), font::load(&v)))
            .collect();
        Ok(Self {
            theme_names,
            themes,
            default_key_area_layout,
            key_area_layouts,
            default_key_set,
            key_sets,
            im_layout_mapping,
            im_font_mapping,
        })
    }

    pub fn theme_names(&self) -> &[String] {
        &self.theme_names
    }

    pub fn theme(&self, name: &str) -> Option<&Theme> {
        self.themes.get(&name.to_ascii_lowercase())
    }

    pub fn key_area_layout(&self, name: &str) -> Rc<KeyAreaLayout> {
        if let Some(l) = self.key_area_layouts.get(name) {
            l.clone()
        } else {
            tracing::debug!("KeyAreaLayout[{}] not found, default is used", name);
            self.default_key_area_layout.clone()
        }
    }

    pub fn key(&self, key_id: &KeyId) -> Option<&Key> {
        let key_set = if let Some(key_set) = &key_id.key_set() {
            match self.key_sets.get(key_set) {
                Some(key_set) => key_set,
                None => {
                    tracing::warn!("key_set[{}] not found, default is used", key_set);
                    &self.default_key_set
                }
            }
        } else {
            &self.default_key_set
        };
        key_set.keys().get(key_id.key_name())
    }

    pub fn font_by_im(&self, im_name: &str) -> Font {
        self.im_font_mapping
            .get(im_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn key_area_layout_by_im(&self, im_name: &str) -> Rc<KeyAreaLayout> {
        if let Some(layout_name) = self.im_layout_mapping.get(im_name) {
            self.key_area_layout(&layout_name)
        } else {
            self.default_key_area_layout.clone()
        }
    }
}

fn init_confs<'de, K, V>(dir_paths: &[PathBuf]) -> Result<HashMap<K, Rc<V>>>
where
    V: IdAndConfigPath<IdType = K> + Deserialize<'de>,
    K: Clone + Display + Eq + Hash,
{
    let mut m = HashMap::<K, Rc<V>>::new();
    for dir_path in dir_paths {
        if !dir_path.exists() {
            continue;
        }
        for file in dir_path.read_dir()? {
            let file = file?;
            if let Some("toml") = file.path().extension().and_then(|p| p.to_str()) {
                let figment = Figment::new().merge(Toml::file(file.path()));
                let mut new: V = figment.extract()?;
                new.set_path(file.path());
                let new = Rc::new(new);
                m.entry(new.id().clone())
                    .and_modify(|old| {
                        tracing::warn!(
                            "duplicate configs for id: {}, {:?} and {:?}, later will be used",
                            old.id(),
                            old.path(),
                            new.path()
                        );
                        *old = new.clone();
                    })
                    .or_insert(new.clone());
            }
        }
    }
    Ok(m)
}

fn init_default<'de, T>(s: &str) -> Result<T>
where
    T: Deserialize<'de>,
{
    let figment = Figment::new().merge(Toml::string(s));
    Ok(figment.extract()?)
}
