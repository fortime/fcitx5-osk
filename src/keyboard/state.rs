use std::{collections::HashMap, mem, rc::Rc};

use anyhow::{bail, Result};
use iced::{
    alignment::{Horizontal, Vertical},
    widget::{Button, Column, Row, Text},
    Element, Padding, Size,
};

use crate::{
    key_set::Key,
    layout::{KeyAreaLayout, KeyManager},
    store::Store,
};

use super::Message;

pub struct State {
    pub layout: LayoutState,
    pub keyboard: KeyboardState,
}

impl State {
    pub fn update_key_area_layout(
        &mut self,
        key_area_layout: Rc<KeyAreaLayout>,
        store: &Store,
    ) -> bool {
        self.keyboard = KeyboardState::new(&key_area_layout, store);
        self.layout.update_key_area_layout(key_area_layout)
    }
}

pub struct LayoutState {
    size_p: (u16, u16),
    unit: u16,
    padding: Padding,
    toolbar_layout: (),
    key_area_layout: Rc<KeyAreaLayout>,
}

impl LayoutState {
    const MIN_P: u16 = 640;

    const MIN_PADDING_P: u16 = 5;

    const TOOLBAR_HEIGHT: u16 = 6;

    pub fn new(width_p: u16, key_area_layout: Rc<KeyAreaLayout>) -> Result<Self> {
        let mut res = Self {
            size_p: (width_p, 0),
            unit: Default::default(),
            padding: Default::default(),
            toolbar_layout: Default::default(),
            key_area_layout,
        };
        res.calculate_size()?;
        Ok(res)
    }

    fn calculate_size(&mut self) -> Result<()> {
        let width_p = self.size_p.0;
        if width_p < Self::MIN_P {
            bail!("width is too small: {}", width_p);
        }
        let trimmed_width_p = width_p - Self::MIN_PADDING_P * 2;
        let unit = self.key_area_layout.unit_within(trimmed_width_p);
        let key_area_size_p = self.key_area_layout.size_p(unit);

        self.unit = unit;
        self.size_p = (
            width_p,
            key_area_size_p.1 + Self::MIN_PADDING_P * 2 + (Self::TOOLBAR_HEIGHT + 1) * unit,
        );
        self.padding = Padding::from([
            Self::MIN_PADDING_P as f32,
            (width_p - key_area_size_p.0) as f32 / 2.0,
        ]);
        Ok(())
    }

    pub fn size(&self) -> Size {
        Size::from((self.size_p.0 as f32, self.size_p.1 as f32))
    }

    pub fn update_width(&mut self, mut width_p: u16) -> bool {
        mem::swap(&mut self.size_p.0, &mut width_p);
        if let Err(e) = self.calculate_size() {
            tracing::debug!("failed to update width: {e}, recovering.");
            // recover
            mem::swap(&mut self.size_p.0, &mut width_p);
            false
        } else {
            true
        }
    }

    fn update_key_area_layout(&mut self, mut key_area_layout: Rc<KeyAreaLayout>) -> bool {
        mem::swap(&mut self.key_area_layout, &mut key_area_layout);
        if let Err(e) = self.calculate_size() {
            tracing::debug!(
                "failed to update key area layout[{}]: {e}, recovering.",
                key_area_layout.name()
            );
            // recover
            mem::swap(&mut self.key_area_layout, &mut key_area_layout);
            false
        } else {
            true
        }
    }

    pub fn to_element<'a, 'b, KM, M>(&'a self, input: &'b str, manager: &'b KM) -> Column<'b, M>
    where
        KM: KeyManager<Message = M>,
        M: 'static,
    {
        Column::new()
            .align_x(Horizontal::Center)
            .padding(self.padding)
            .push(Text::new(input).height(Self::TOOLBAR_HEIGHT * self.unit))
            .push(self.key_area_layout.to_element(self.unit, manager))
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum ModifierState {
    NoState = 0x0,
    Shift = 1 << 0,
    CapsLock = 1 << 1,
    Ctrl = 1 << 2,
    Alt = 1 << 3,
    NumLock = 1 << 4,
    Super = 1 << 6,
    Virtual = 1 << 29,
    Repeat = 1 << 31,
}

impl ModifierState {
    pub fn is_pressed(&self, state: u32) -> bool {
        *self as u32 & state != 0
    }
}

pub struct KeyboardState {
    input: String,
    modifiers: u32,
    primary_text_size: u16,
    secondary_text_size: u16,
    keys: HashMap<String, Key>,
}

impl KeyboardState {
    pub fn new(key_area_layout: &KeyAreaLayout, store: &Store) -> Self {
        let keys = key_area_layout
            .key_mappings()
            .iter()
            .filter_map(|(k, v)| store.key(v).map(|key| (k.clone(), key.clone())))
            .collect();
        Self {
            input: String::new(),
            // always virtual
            modifiers: ModifierState::Virtual as u32,
            primary_text_size: *key_area_layout.primary_text_size(),
            secondary_text_size: *key_area_layout.secondary_text_size(),
            keys,
        }
    }

    pub fn update_input(&mut self, s: String) {
        self.input.push_str(&s);
    }

    pub fn input(&self) -> &str {
        &self.input
    }
}

impl KeyManager for KeyboardState {
    type Message = Message;

    fn key<'a, 'b>(
        &'a self,
        key_name: &'b str,
        unit: u16,
        width_p: u16,
        height_p: u16,
    ) -> Element<'a, Self::Message> {
        let (content, pressed_message) = if let Some(key) = self.keys.get(key_name) {
            let is_shift_pressed = ModifierState::Shift.is_pressed(self.modifiers);
            let secondary_height_p = height_p / 4;
            let primary_height_p = height_p - 2 * secondary_height_p;
            let mut column = Column::new();
            let (top, middle) = if is_shift_pressed {
                if let Some(secondary) = key.secondaries().get(0) {
                    (Text::new(""), Text::new(secondary.symbol()))
                } else {
                    (Text::new(""), Text::new(key.primary().symbol()))
                }
            } else {
                (
                    Text::new(
                        key.secondaries()
                            .get(0)
                            .map(|s| s.symbol().as_str())
                            .unwrap_or(""),
                    ),
                    Text::new(key.primary().symbol()),
                )
            };
            column = column
                .push(
                    top.width(width_p)
                        .height(secondary_height_p)
                        .size((self.secondary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Right),
                )
                .push(
                    middle
                        .width(width_p)
                        .height(primary_height_p)
                        .size((self.primary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            (
                Element::from(column),
                Some(Message::KeyPressed(key_name.to_string())),
            )
        } else {
            tracing::debug!("{key_name} is not found");
            (Element::from(Text::new("")), None)
        };
        Button::new(content)
            .width(width_p)
            .height(height_p)
            .on_press_maybe(pressed_message)
            .into()
    }
}
