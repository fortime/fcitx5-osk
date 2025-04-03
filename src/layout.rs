//! In this layout, the unit of length is not pixel or meter. It is 1/8 of a normal
//! key's width.

use getset::{CopyGetters, Getters};
use iced::{
    advanced::svg::Handle as SvgHandle,
    alignment::{Horizontal, Vertical},
    widget::{
        button::Style as ButtonStyle,
        scrollable::{Direction, Scrollbar},
        Button, Column, Container, PickList, Row, Scrollable, Space, Svg, Text,
    },
    window::Id,
    Color, Element, Font, Length, Theme, Vector,
};
use iced_font_awesome::{FaIcon, IconFont};
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};

use std::{
    collections::HashMap, marker::PhantomData, path::PathBuf, result::Result as StdResult,
    sync::Arc, time::Duration,
};

use crate::{
    state::CandidateAreaState,
    store::IdAndConfigPath,
    widget::{Movable, Toggle, ToggleCondition},
};

pub trait KeyManager {
    type Message;

    fn key(&self, key_name: Arc<str>, unit: u16, size: (u16, u16)) -> Element<Self::Message>;

    fn popup_overlay(&self, unit: u16, size: (u16, u16)) -> Option<Element<Self::Message>>;
}

pub trait KeyboardManager {
    type Message;

    fn nothing() -> Self::Message;

    fn available_candidate_width(&self) -> u16;

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

    fn open_keyboard(&self) -> Self::Message;

    fn close_keyboard(&self) -> Self::Message;

    fn open_indicator(&self) -> Option<Self::Message>;

    fn new_position(&self, id: Id, delta: Vector) -> Option<Self::Message>;

    fn set_movable(&self, id: Id, movable: bool) -> Self::Message;
}

#[derive(Deserialize, CopyGetters, Getters)]
pub struct KeyAreaLayout {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    /// vertical space between rows
    #[serde(alias = "spacing", default = "KeyAreaLayout::default_spacing_u")]
    spacing_u: u16,
    elements: Vec<KeyRow>,
    #[getset(get = "pub")]
    key_mappings: HashMap<String, KeyId>,
    #[serde(
        alias = "primary_text_size",
        default = "KeyAreaLayout::default_primary_text_size_u"
    )]
    #[getset(get_copy = "pub")]
    primary_text_size_u: u16,
    #[serde(
        alias = "secondary_text_size",
        default = "KeyAreaLayout::default_secondary_text_size_u"
    )]
    #[getset(get_copy = "pub")]
    secondary_text_size_u: u16,
    #[serde(
        alias = "popup_key_width",
        default = "KeyAreaLayout::default_popup_key_width_u"
    )]
    #[getset(get_copy = "pub")]
    popup_key_width_u: u16,
    #[serde(
        alias = "popup_key_height",
        default = "KeyAreaLayout::default_popup_key_height_u"
    )]
    #[getset(get_copy = "pub")]
    popup_key_height_u: u16,
    #[getset(get = "pub")]
    font: Option<String>,
}

impl KeyAreaLayout {
    fn default_spacing_u() -> u16 {
        1
    }

    fn default_primary_text_size_u() -> u16 {
        2
    }

    fn default_popup_key_width_u() -> u16 {
        4
    }

    fn default_popup_key_height_u() -> u16 {
        4
    }

    fn default_secondary_text_size_u() -> u16 {
        1
    }

    pub fn width_u(&self) -> u16 {
        self.elements.iter().map(KeyRow::width_u).max().unwrap_or(0)
    }

    pub fn height_u(&self) -> u16 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut height_u = self.spacing_u * (self.elements.len() as u16 - 1);
        height_u += self.elements.iter().map(KeyRow::height_u).sum::<u16>();
        height_u
    }

    pub fn size(&self, unit: u16) -> (u16, u16) {
        (self.width_u() * unit, self.height_u() * unit)
    }

    pub fn to_element<'a, 'b, KM, M>(
        &'a self,
        unit: u16,
        manager: &'b KM,
    ) -> impl Into<Element<'b, M>>
    where
        KM: KeyManager<Message = M>,
        M: 'b,
    {
        let mut col = Column::new()
            .spacing(self.spacing_u * unit)
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
    #[serde(alias = "height")]
    height_u: u16,
    #[serde(alias = "spacing")]
    /// horizontal space between elements
    spacing_u: u16,
    elements: Vec<KeyRowElement>,
}

impl KeyRow {
    fn width_u(&self) -> u16 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut width_u = self.spacing_u * (self.elements.len() as u16 - 1);
        width_u += self
            .elements
            .iter()
            .map(KeyRowElement::width_u)
            .sum::<u16>();
        width_u
    }

    fn height_u(&self) -> u16 {
        self.height_u
    }

    fn to_element<'a, 'b, KM, M>(&'a self, unit: u16, manager: &'b KM) -> impl Into<Element<'b, M>>
    where
        KM: KeyManager<Message = M>,
        M: 'b,
    {
        let mut row = Row::new()
            .spacing(self.spacing_u * unit)
            .align_y(Vertical::Center)
            .height(self.height_u * unit);
        for element in &self.elements {
            row = row.push(element.to_element(self.height_u, unit, manager));
        }
        row
    }
}

pub enum KeyRowElement {
    Padding(u16),
    Key {
        width_u: u16,
        height_u: Option<u16>,
        name: Arc<str>,
    },
}

impl KeyRowElement {
    fn width_u(&self) -> u16 {
        match self {
            KeyRowElement::Padding(n) => *n,
            KeyRowElement::Key {
                width_u,
                height_u: _height,
                name: _name,
            } => *width_u,
        }
    }

    fn to_element<'a, 'b, KM, M>(
        &'a self,
        max_height_u: u16,
        unit: u16,
        manager: &'b KM,
    ) -> Element<'b, M>
    where
        KM: KeyManager<Message = M>,
        M: 'b,
    {
        match self {
            KeyRowElement::Padding(width_u) => Space::with_width(width_u * unit).into(),
            KeyRowElement::Key {
                width_u,
                height_u,
                name,
            } => {
                let mut height_u = height_u.unwrap_or(max_height_u);
                if height_u > max_height_u {
                    tracing::warn!(
                        "key[{name}] is too high: {height_u}, updated to: {}",
                        max_height_u
                    );
                    height_u = max_height_u;
                }
                tracing::trace!(
                    "key: {}, width: {}, height: {}",
                    name,
                    width_u * unit,
                    height_u * unit
                );
                manager
                    .key(name.clone(), unit, (width_u * unit, height_u * unit))
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
            let width_u = items.next().unwrap_or("1");
            match width_u.parse() {
                Ok(n) => Ok(KeyRowElement::Padding(n)),
                Err(_) => Err(Error::invalid_value(
                    Unexpected::Str(width_u),
                    &"it should end with an empty string or a u16 integer",
                )),
            }
        } else if typ.starts_with("k") {
            let width_u = items.next().unwrap_or("8");
            let width_u = match width_u.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(Error::invalid_value(
                        Unexpected::Str(width_u),
                        &"width should be empty or a u16 integer",
                    ))
                }
            };
            let height_u = items
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
                height_u,
                width_u,
                name: typ.into(),
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

    pub fn to_element<'a, 'b, KbdM, KM, M>(
        &'a self,
        params: ToElementCommonParams<'b, KbdM, KM, M>,
        unit: u16,
        candidate_font: Font,
        font_size_u: u16,
    ) -> Element<'b, M>
    where
        KbdM: KeyboardManager<Message = M>,
        M: 'b + Clone,
    {
        if params.candidate_area_state.has_candidate() {
            self.to_candidate_element(params, unit, candidate_font, font_size_u)
        } else {
            self.to_toolbar_element(params, unit, font_size_u)
        }
    }

    fn to_candidate_element<'a, 'b, KbdM, KM, M>(
        &'a self,
        params: ToElementCommonParams<'b, KbdM, KM, M>,
        unit: u16,
        font: Font,
        font_size_u: u16,
    ) -> Element<'b, M>
    where
        KbdM: KeyboardManager<Message = M>,
        M: 'b + Clone,
    {
        let theme = params.theme;
        let keyboard_manager = params.keyboard_manager;
        let state = params.candidate_area_state;
        let spacing = 2 * unit;
        let font_size = font_size_u * unit;
        let color = theme.extended_palette().background.weak.text;
        let disabled_color = theme.extended_palette().background.weak.color;

        let mut available_candidate_width = keyboard_manager.available_candidate_width();
        // minus the size of < and > and their spacing
        available_candidate_width -= 2 * font_size;
        let candidate_list = state.candidate_list();
        let mut candidate_row = Row::new().spacing(spacing).align_y(Vertical::Center);
        let char_width = font_size;
        let (consumed, max_width) = if state.is_paged() {
            (
                candidate_list.len(),
                candidate_list
                    .iter()
                    .map(|c| c.chars().count())
                    .max()
                    .unwrap_or(0) as u16
                    * char_width,
            )
        } else {
            let mut consumed = 0;
            let mut max_width = 0;
            for candidate in candidate_list {
                // TODO Simply assume one char consumes 2 * font_size. Calculate the width in the
                // future.
                let width = candidate.chars().count() as u16;
                if max_width.max(width) * (consumed + 1) * char_width + consumed * spacing
                    > available_candidate_width
                {
                    break;
                }
                max_width = max_width.max(width);
                consumed += 1;
            }
            (consumed as usize, max_width * char_width)
        };
        let mut index = state.cursor();
        // as least 1
        for candidate in &candidate_list[..consumed.max(1)] {
            candidate_row = candidate_row.push(
                candidate_btn(&candidate, font, font_size, max_width)
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
                font_size,
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
                font_size,
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

    fn to_toolbar_element<'a, 'b, KbdM, KM, M>(
        &'a self,
        params: ToElementCommonParams<'b, KbdM, KM, M>,
        unit: u16,
        font_size_u: u16,
    ) -> Element<'b, M>
    where
        KbdM: KeyboardManager<Message = M>,
        M: 'b + Clone,
    {
        let theme = params.theme;
        let keyboard_manager = params.keyboard_manager;
        let color = theme.extended_palette().background.weak.text;
        let font_size = font_size_u * unit;
        let mut row = Row::new()
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .spacing(unit * 2);

        if let Some(message) = keyboard_manager.open_indicator() {
            row = row.push(
                fa_btn(
                    "down-left-and-up-right-to-center",
                    IconFont::Solid,
                    font_size,
                    color,
                )
                .on_press(message),
            );
        }

        // padding
        let window_id = params.window_id;
        let movable = params.movable;
        row = row.push(
            Toggle::new(
                Movable::new(
                    Column::new()
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .push(Text::new(" ")),
                    move |delta| {
                        keyboard_manager
                            .new_position(window_id, delta)
                            .unwrap_or_else(KbdM::nothing)
                    },
                    movable,
                )
                .on_move_end(keyboard_manager.set_movable(window_id, false)),
                ToggleCondition::LongPress(Duration::from_millis(1000)),
            )
            .on_toggle(keyboard_manager.set_movable(window_id, !movable)),
        );

        row = row.push(
            Row::new()
                .align_y(Vertical::Center)
                .spacing(unit)
                .push(
                    FaIcon::new("globe", IconFont::Solid)
                        .size(font_size)
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
                        .size(font_size)
                        .color(color),
                )
                .push(PickList::new(
                    keyboard_manager.themes(),
                    Some(keyboard_manager.selected_theme()),
                    |theme| keyboard_manager.select_theme(&theme),
                )),
        );
        row = row.push(
            fa_btn("gear", IconFont::Solid, font_size, color)
                .on_press_with(|| keyboard_manager.toggle_setting()),
        );
        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Right)
            .into()
    }
}

pub(crate) struct ToElementCommonParams<'a, KbdM, KM, M> {
    pub candidate_area_state: &'a CandidateAreaState,
    pub keyboard_manager: &'a KbdM,
    pub key_manager: &'a KM,
    pub theme: &'a Theme,
    pub window_id: Id,
    pub movable: bool,
    pub phantom: PhantomData<M>,
}

fn fa_btn<Message>(
    name: &str,
    icon_font: IconFont,
    font_size: u16,
    color: Color,
) -> Button<Message> {
    Button::new(FaIcon::new(name, icon_font).size(font_size).color(color))
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}

fn candidate_btn<Message>(
    candidate: &str,
    font: Font,
    font_size: u16,
    width: u16,
) -> Button<Message> {
    let text = Text::new(candidate)
        .font(font)
        .size(font_size)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center);
    Button::new(text)
        .width(width)
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}

pub fn indicator_btn<'a, Message>(width: u16) -> Button<'a, Message>
where
    Message: 'a,
{
    let icon = include_bytes!("../assets/icons/fcitx5-osk.svg");
    let svg = Svg::new(SvgHandle::from_memory(icon)).width(width);
    Button::new(svg)
        .width(width)
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}
