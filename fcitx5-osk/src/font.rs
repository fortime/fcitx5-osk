use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use iced::Font;

static FONTS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
static DEFAULT_NERD_FONT: Mutex<Font> = Mutex::new(Font::with_name("NotoSerif NF"));

fn fonts() -> &'static Mutex<HashMap<String, &'static str>> {
    FONTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn set_default_nerd_font(name: &str) {
    // To avoid deadlock, load the font, then set it
    let font = load(name);
    *DEFAULT_NERD_FONT
        .lock()
        .expect("DEFAULT_NERD_FONT is poisoned") = font;
}

pub fn load(name: &str) -> Font {
    if name == "fcitx5 osk nerd" {
        return *DEFAULT_NERD_FONT
            .lock()
            .expect("DEFAULT_NERD_FONT is poisoned");
    }
    let mut fonts = fonts().lock().expect("FONTS is poisoned");
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
