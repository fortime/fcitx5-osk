use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use iced::Font;

static FONTS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

fn fonts() -> &'static Mutex<HashMap<String, &'static str>> {
    FONTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn load(name: &str) -> Font {
    let mut fonts = fonts().lock().expect("FONTS shouldn't be poisoned");
    let static_name = match fonts.get(name) {
        Some(static_name) => *static_name,
        None => {
            let name = name.to_string();
            let static_name: &'static str = name.to_string().leak();
            fonts.insert(name, static_name);
            static_name
        }
    };
    Font::with_name(static_name)
}
