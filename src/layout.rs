//! In this layout, the unit of length is not pixel or meter. It is 1/8 of a normal
//! key's width.

use getset::{CopyGetters, Getters};
use iced::{
    alignment::{Horizontal, Vertical},
    widget::{
        button::Style as ButtonStyle,
        scrollable::{Direction, Scrollbar},
        Button, Column, Container, PickList, Row, Scrollable, Space, Text,
    },
    Color, Element, Font, Length, Theme,
};
use iced_font_awesome::{FaIcon, IconFont};
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};

use std::{collections::HashMap, path::PathBuf, result::Result as StdResult, sync::Arc};

use crate::{state::CandidateAreaState, store::IdAndConfigPath};

pub trait KeyManager {
    type Message;

    fn key(&self, key_name: Arc<String>, unit: u16, size_p: (u16, u16)) -> Element<Self::Message>;

    fn popup_overlay(&self, unit: u16, size_p: (u16, u16)) -> Option<Element<Self::Message>>;
}

pub trait KeyboardManager {
    type Message;

    fn available_candidate_width_p(&self) -> u16;

    fn themes(&self) -> &[String];

    fn selected_theme(&self) -> &String;

    fn select_theme(&self, theme: &String) -> Self::Message;

    fn ims(&self) -> &[String];

    fn selected_im(&self) -> Option<&String>;

    fn select_im(&self, im: &String) -> Self::Message;

    fn toggle_setting(&self) -> Self::Message;

    fn prev_candidates_message(&self) -> Self::Message;

    fn next_candidates_message(&self, cursor: usize) -> Self::Message;

    fn select_candidate_message(&self, index: usize) -> Self::Message;
}

#[derive(Deserialize, CopyGetters, Getters)]
pub struct KeyAreaLayout {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    /// vertical space between rows
    #[serde(default = "KeyAreaLayout::default_spacing")]
    spacing: u16,
    elements: Vec<KeyRow>,
    #[getset(get = "pub")]
    key_mappings: HashMap<String, KeyId>,
    #[serde(default = "KeyAreaLayout::default_primary_text_size")]
    #[getset(get_copy = "pub")]
    primary_text_size: u16,
    #[serde(default = "KeyAreaLayout::default_secondary_text_size")]
    #[getset(get_copy = "pub")]
    secondary_text_size: u16,
    #[serde(default = "KeyAreaLayout::default_popup_key_width")]
    #[getset(get_copy = "pub")]
    popup_key_width: u16,
    #[serde(default = "KeyAreaLayout::default_popup_key_height")]
    #[getset(get_copy = "pub")]
    popup_key_height: u16,
    #[getset(get = "pub")]
    font: Option<String>,
}

impl KeyAreaLayout {
    fn default_spacing() -> u16 {
        1
    }

    fn default_primary_text_size() -> u16 {
        2
    }

    fn default_popup_key_width() -> u16 {
        4
    }

    fn default_popup_key_height() -> u16 {
        4
    }

    fn default_secondary_text_size() -> u16 {
        1
    }

    pub fn width(&self) -> u16 {
        self.elements.iter().map(KeyRow::width).max().unwrap_or(0)
    }

    pub fn height(&self) -> u16 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut height = self.spacing * (self.elements.len() as u16 - 1);
        height += self.elements.iter().map(KeyRow::height).sum::<u16>();
        height
    }

    pub fn size_p(&self, unit: u16) -> (u16, u16) {
        (self.width() * unit, self.height() * unit)
    }

    pub fn to_element<'a, 'b, KM, M>(
        &'a self,
        unit: u16,
        manager: &'b KM,
    ) -> impl Into<Element<'b, M>>
    where
        KM: KeyManager<Message = M>,
        M: 'static,
    {
        let mut col = Column::new()
            .spacing(self.spacing * unit)
            .align_x(Horizontal::Center);

        for key_row in &self.elements {
            col = col.push(key_row.to_element(unit, manager));
        }

        col
    }
}

impl IdAndConfigPath for KeyAreaLayout {
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

#[derive(Getters)]
pub struct KeyId {
    #[getset(get = "pub")]
    key_set: Option<String>,
    #[getset(get = "pub")]
    key_name: String,
}

impl<'de> Deserialize<'de> for KeyId {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        let items: Vec<_> = s.splitn(3, ':').collect();
        match items.len() {
            0 => unreachable!("it shouldn't be empty after split"),
            1 => Ok(Self {
                key_set: None,
                key_name: items[0].to_string(),
            }),
            2 => Ok(Self {
                key_set: Some(items[0].to_string()),
                key_name: items[1].to_string(),
            }),
            3.. => Err(Error::invalid_value(
                Unexpected::Char(':'),
                &"the value contains more than one ':'",
            )),
        }
    }
}

#[derive(Deserialize)]
pub struct KeyRow {
    height: u16,
    /// horizontal space between elements
    spacing: u16,
    elements: Vec<KeyRowElement>,
}

impl KeyRow {
    fn width(&self) -> u16 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut width = self.spacing * (self.elements.len() as u16 - 1);
        width += self.elements.iter().map(KeyRowElement::width).sum::<u16>();
        width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn to_element<'a, 'b, KM, M>(&'a self, unit: u16, manager: &'b KM) -> impl Into<Element<'b, M>>
    where
        KM: KeyManager<Message = M>,
        M: 'static,
    {
        let mut row = Row::new()
            .spacing(self.spacing * unit)
            .align_y(Vertical::Center)
            .height(self.height * unit);
        for element in &self.elements {
            row = row.push(element.to_element(self.height, unit, manager));
        }
        row
    }
}

pub enum KeyRowElement {
    Padding(u16),
    Key {
        width: u16,
        height: Option<u16>,
        name: Arc<String>,
    },
}

impl KeyRowElement {
    fn width(&self) -> u16 {
        match self {
            KeyRowElement::Padding(n) => *n,
            KeyRowElement::Key {
                width,
                height: _height,
                name: _name,
            } => *width,
        }
    }

    fn to_element<'a, 'b, KM, M>(
        &'a self,
        max_height: u16,
        unit: u16,
        manager: &'b KM,
    ) -> Element<'b, M>
    where
        KM: KeyManager<Message = M>,
        M: 'static,
    {
        match self {
            KeyRowElement::Padding(width) => Space::with_width(width * unit).into(),
            KeyRowElement::Key {
                width,
                height,
                name,
            } => {
                let mut height = height.unwrap_or(max_height);
                if height > max_height {
                    tracing::warn!(
                        "key[{name}] is too high: {height}, updated to: {}",
                        max_height
                    );
                    height = max_height;
                }
                tracing::trace!(
                    "key: {}, width: {}, height: {}",
                    name,
                    width * unit,
                    height * unit
                );
                manager
                    .key(name.clone(), unit, (width * unit, height * unit))
                    .into()
            }
        }
    }
}

impl<'de> Deserialize<'de> for KeyRowElement {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        let mut items = s.splitn(2, ':');
        let typ = items.next().unwrap_or("");
        if typ == "p" {
            let width = items.next().unwrap_or("1");
            match width.parse() {
                Ok(n) => Ok(KeyRowElement::Padding(n)),
                Err(_) => Err(Error::invalid_value(
                    Unexpected::Str(width),
                    &"it should end with an empty string or a u16 integer",
                )),
            }
        } else if typ.starts_with("k") {
            let width = items.next().unwrap_or("8");
            let width = match width.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(Error::invalid_value(
                        Unexpected::Str(width),
                        &"width should be empty or a u16 integer",
                    ))
                }
            };
            let height = items
                .next()
                .map(|s| {
                    s.parse().map_err(|_| {
                        Error::invalid_value(
                            Unexpected::Str(s),
                            &"height should be empty or a u16 integer",
                        )
                    })
                })
                .transpose()?;
            Ok(KeyRowElement::Key {
                height,
                width,
                name: Arc::new(typ.to_string()),
            })
        } else {
            Err(Error::invalid_value(
                Unexpected::Str(typ),
                &"it starts with 'p' or 'k'",
            ))
        }
    }
}

pub struct ToolbarLayout {
    height: u16,
}

impl ToolbarLayout {
    pub fn new() -> Self {
        Self { height: 6 }
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn to_element<'a, 'b, KM, M>(
        &'a self,
        keyboard_manager: &'b KM,
        unit: u16,
        candidate_area_state: &'b CandidateAreaState,
        candidate_font: Font,
        font_size: u16,
        theme: &'a Theme,
    ) -> Element<'b, M>
    where
        KM: KeyboardManager<Message = M>,
        M: 'static + Clone,
    {
        if candidate_area_state.has_candidate() {
            self.to_candidate_element(
                keyboard_manager,
                unit,
                candidate_area_state,
                candidate_font,
                font_size,
                theme,
            )
        } else {
            self.to_toolbar_element(keyboard_manager, unit, font_size, theme)
        }
    }

    fn to_candidate_element<'a, 'b, KM, M>(
        &'a self,
        keyboard_manager: &'b KM,
        unit: u16,
        state: &'b CandidateAreaState,
        font: Font,
        font_size: u16,
        theme: &'a Theme,
    ) -> Element<'b, M>
    where
        KM: KeyboardManager<Message = M>,
        M: 'static + Clone,
    {
        let spacing_p = 2 * unit;
        let font_size_p = font_size * unit;
        let color = theme.extended_palette().background.weak.text;
        let disabled_color = theme.extended_palette().background.weak.color;

        let mut available_candidate_width_p = keyboard_manager.available_candidate_width_p();
        // minus the size of < and > and their spacing
        available_candidate_width_p -= 2 * font_size_p;
        let candidate_list = state.candidate_list();
        let mut candidate_row = Row::new().spacing(spacing_p).align_y(Vertical::Center);
        let char_width_p = 2 * font_size_p;
        let (consumed, max_width_p) = if state.is_paged() {
            (
                candidate_list.len(),
                candidate_list
                    .iter()
                    .map(|c| c.chars().count())
                    .max()
                    .unwrap_or(0) as u16
                    * char_width_p,
            )
        } else {
            let mut consumed = 0;
            let mut max_width = 0;
            for candidate in candidate_list {
                // TODO Simply assume one char consumes 2 * font_size_p. Calculate the width in the
                // future.
                let width = candidate.chars().count() as u16;
                if max_width.max(width) * (consumed + 1) * char_width_p + consumed * spacing_p
                    > available_candidate_width_p
                {
                    break;
                }
                max_width = max_width.max(width);
                consumed += 1;
            }
            (consumed as usize, max_width * char_width_p)
        };
        let mut index = state.cursor();
        // as least 1
        for candidate in &candidate_list[..consumed.max(1)] {
            candidate_row = candidate_row.push(
                candidate_btn(&candidate, font, font_size_p, max_width_p)
                    .on_press(keyboard_manager.select_candidate_message(index)),
            );
            index += 1;
        }

        let prev_message = if state.cursor() > 0 || state.has_prev_in_fcitx5() {
            Some(keyboard_manager.prev_candidates_message())
        } else {
            None
        };

        let next_message = if consumed < candidate_list.len() || state.has_next_in_fcitx5() {
            Some(keyboard_manager.next_candidates_message(consumed + state.cursor()))
        } else {
            None
        };
        let candidate_element: Element<_> = if state.is_paged() {
            Scrollable::with_direction(
                candidate_row,
                Direction::Horizontal(Scrollbar::new().width(1).spacing(unit)),
            )
            .into()
        } else {
            candidate_row.into()
        };

        let mut row = Row::new().height(Length::Fill).align_y(Vertical::Center);
        row = row.push(
            fa_btn(
                "caret-left",
                IconFont::Solid,
                font_size_p,
                if prev_message.is_some() {
                    color
                } else {
                    disabled_color
                },
            )
            .on_press_maybe(prev_message),
        );
        row = row.push(
            // make it scrollable if there are too many items
            Container::new(candidate_element).center(Length::Fill),
        );
        row = row.push(
            fa_btn(
                "caret-right",
                IconFont::Solid,
                font_size_p,
                if next_message.is_some() {
                    color
                } else {
                    disabled_color
                },
            )
            .on_press_maybe(next_message),
        );

        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .into()
    }

    fn to_toolbar_element<'a, 'b, KM, M>(
        &'a self,
        keyboard_manager: &'b KM,
        unit: u16,
        font_size: u16,
        theme: &'a Theme,
    ) -> Element<'b, M>
    where
        KM: KeyboardManager<Message = M>,
        M: 'static + Clone,
    {
        let color = theme.extended_palette().background.weak.text;
        let font_size_p = font_size * unit;
        let mut row = Row::new()
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .spacing(unit * 2);

        row = row.push(
            Row::new()
                .align_y(Vertical::Center)
                .spacing(unit)
                .push(
                    FaIcon::new("globe", IconFont::Solid)
                        .size(font_size_p)
                        .color(color),
                )
                .push(PickList::new(
                    keyboard_manager.ims(),
                    keyboard_manager.selected_im(),
                    |im| keyboard_manager.select_im(&im),
                )),
        );
        row = row.push(
            Row::new()
                .align_y(Vertical::Center)
                .spacing(unit)
                .push(
                    FaIcon::new("palette", IconFont::Solid)
                        .size(font_size_p)
                        .color(color),
                )
                .push(PickList::new(
                    keyboard_manager.themes(),
                    Some(keyboard_manager.selected_theme()),
                    |theme| keyboard_manager.select_theme(&theme),
                )),
        );
        row = row.push(
            fa_btn("gear", IconFont::Solid, font_size_p, color)
                .on_press_with(|| keyboard_manager.toggle_setting()),
        );
        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Right)
            .into()
    }
}

fn fa_btn<Message>(
    name: &str,
    icon_font: IconFont,
    font_size_p: u16,
    color: Color,
) -> Button<Message> {
    Button::new(FaIcon::new(name, icon_font).size(font_size_p).color(color))
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}

fn candidate_btn<Message>(
    candidate: &str,
    font: Font,
    font_size_p: u16,
    width_p: u16,
) -> Button<Message> {
    let text = Text::new(candidate)
        .font(font)
        .size(font_size_p)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center);
    Button::new(text)
        .width(width_p)
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}
