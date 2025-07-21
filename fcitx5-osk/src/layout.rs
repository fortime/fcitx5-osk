//! In this layout, the unit of length is not pixel or meter. It is 1/8 of a normal
//! key's width.

use getset::{CopyGetters, Getters};
use iced::{
    advanced::svg::Handle as SvgHandle,
    alignment::{Horizontal, Vertical},
    widget::{
        button::Style as ButtonStyle,
        scrollable::{Direction, Scrollbar},
        text::Shaping,
        text_input::TextInput,
        Button, Column, Container, PickList, Row, Scrollable, Space, Svg, Text,
    },
    window::Id,
    Color, Element, Font, Length, Padding,
};
//use iced_font_awesome::{FaIcon, IconFont};
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};

use std::{
    collections::HashMap, path::PathBuf, result::Result as StdResult, sync::Arc, time::Duration,
};

use crate::{
    app::Message,
    config::IndicatorDisplay,
    state::{
        CloseOpSource, DynamicEnumDesc, EnumDesc, Field, FieldType, ImEvent, LayoutEvent,
        OwnedEnumDesc, StateExtractor, StepDesc, TextDesc, UpdateConfigEvent, WindowEvent,
        WindowManagerEvent,
    },
    store::IdAndConfigPath,
    widget::{Movable, Toggle, ToggleCondition},
    window::WindowManagerMode,
};

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
    #[serde(
        alias = "min_toolbar_height",
        default = "KeyAreaLayout::default_min_toolbar_height_u"
    )]
    #[getset(get_copy = "pub")]
    min_toolbar_height_u: u16,
    #[getset(get = "pub")]
    font: Option<String>,
}

impl KeyAreaLayout {
    fn default_spacing_u() -> u16 {
        1
    }

    fn default_primary_text_size_u() -> u16 {
        3
    }

    fn default_popup_key_width_u() -> u16 {
        8
    }

    fn default_popup_key_height_u() -> u16 {
        6
    }

    fn default_secondary_text_size_u() -> u16 {
        2
    }

    fn default_min_toolbar_height_u() -> u16 {
        6
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

    pub fn to_element<'b>(
        &self,
        unit: u16,
        state: &'b dyn StateExtractor,
    ) -> impl Into<Element<'b, Message>> {
        let mut col = Column::new()
            .spacing(self.spacing_u * unit)
            .align_x(Horizontal::Center);

        for key_row in &self.elements {
            col = col.push(key_row.to_element(unit, state));
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

    fn to_element<'b>(
        &self,
        unit: u16,
        state: &'b dyn StateExtractor,
    ) -> impl Into<Element<'b, Message>> {
        let mut row = Row::new()
            .spacing(self.spacing_u * unit)
            .align_y(Vertical::Center)
            .height(self.height_u * unit);
        for element in &self.elements {
            row = row.push(element.to_element(self.height_u, unit, state));
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

    fn to_element<'b>(
        &self,
        max_height_u: u16,
        unit: u16,
        state: &'b dyn StateExtractor,
    ) -> Element<'b, Message> {
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
                state
                    .keyboard()
                    .key(name.clone(), unit, (width_u * unit, height_u * unit))
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
    height_u: u16,
}

impl ToolbarLayout {
    pub fn new(min_toolbar_height_u: u16) -> Self {
        let mut res = Self { height_u: 6 };
        res.update_height_u(min_toolbar_height_u);
        res
    }

    pub fn update_height_u(&mut self, min_toolbar_height_u: u16) {
        self.height_u = 6.max(min_toolbar_height_u)
    }

    pub fn height_u(&self) -> u16 {
        self.height_u
    }

    pub fn to_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: u16,
        candidate_font: Font,
        font_size_u: u16,
    ) -> Element<'b, Message> {
        if params.state.im().candidate_area_state().has_candidate() {
            self.to_candidate_element(params, unit, candidate_font, font_size_u)
        } else {
            self.to_toolbar_element(params, unit, font_size_u)
        }
    }

    fn to_candidate_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: u16,
        font: Font,
        font_size_u: u16,
    ) -> Element<'b, Message> {
        let theme = params.state.theme();
        let state = params.state.im().candidate_area_state();
        let spacing = 2 * unit;
        let font_size = font_size_u * unit;
        let color = theme.extended_palette().background.weak.text;
        let disabled_color = theme.extended_palette().background.weak.color;

        let mut available_candidate_width = params.state.available_candidate_width();
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
                // TODO Simply assume one char consumes 1 * font_size. Calculate the width in the
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
                candidate_btn(candidate, font, font_size, max_width)
                    .on_press(ImEvent::SelectCandidate(index).into()),
            );
            index += 1;
        }

        let prev_message = if state.cursor() > 0 || state.has_prev_in_fcitx5() {
            Some(ImEvent::PrevCandidates.into())
        } else {
            None
        };

        let next_message = if consumed < candidate_list.len() || state.has_next_in_fcitx5() {
            Some(ImEvent::NextCandidates(consumed + state.cursor()).into())
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
            nerd_btn(
                '󰒮',
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
            nerd_btn(
                '󰒭',
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

    fn to_toolbar_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: u16,
        font_size_u: u16,
    ) -> Element<'b, Message> {
        let state = params.state;
        let theme = state.theme();
        let color = theme.extended_palette().background.weak.text;
        let font_size = font_size_u * unit;
        let mut row = Row::new()
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .spacing(unit * 2);

        let indicator_message = match state.indicator_display() {
            IndicatorDisplay::Auto => Some(WindowManagerEvent::OpenIndicator.into()),
            IndicatorDisplay::AlwaysOn => {
                Some(WindowManagerEvent::CloseKeyboard(CloseOpSource::UserAction).into())
            }
            IndicatorDisplay::AlwaysOff => {
                if state.window_manager_mode() == WindowManagerMode::KwinLockScreen {
                    None
                } else {
                    Some(WindowManagerEvent::CloseKeyboard(CloseOpSource::UserAction).into())
                }
            }
        };
        if let Some(message) = indicator_message {
            row = row.push(nerd_btn('󰁄', font_size, color).on_press(message));
        }

        // padding
        let window_id = params.window_id;
        let movable = state.movable(window_id);
        row = row.push(
            Toggle::new(
                Movable::new(
                    Column::new()
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .push(Text::new(" ")),
                    move |delta| {
                        state
                            .new_position_message(window_id, delta)
                            .unwrap_or(Message::Nothing)
                    },
                    movable,
                )
                .on_move_end(WindowEvent::SetMovable(window_id, false).into()),
                ToggleCondition::LongPress(Duration::from_millis(1000)),
            )
            .on_toggle(WindowEvent::SetMovable(window_id, !movable).into()),
        );

        row = row.push(
            Row::new()
                .align_y(Vertical::Center)
                .spacing(unit)
                .push(nerd_icon('󰏪', font_size, color))
                .push(
                    PickList::new(state.im().im_names(), state.im().im_name(), |im| {
                        ImEvent::SelectIm(im).into()
                    })
                    .text_size(font_size),
                ),
        );
        row = row.push(
            Row::new()
                .align_y(Vertical::Center)
                .spacing(unit)
                .push(nerd_icon('󰏘', font_size, color))
                .push(
                    PickList::new(
                        state.store().theme_names(),
                        Some(state.config().theme()),
                        |theme| UpdateConfigEvent::Theme(theme).into(),
                    )
                    .text_size(font_size),
                ),
        );
        row = row.push(nerd_btn('󰘮', font_size, color).on_press(LayoutEvent::ToggleSetting.into()));
        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Right)
            .padding(Padding::from([0, unit]))
            .into()
    }
}

pub struct SettingLayout;

impl SettingLayout {
    pub fn to_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: u16,
        font_size_u: u16,
    ) -> Element<'b, Message> {
        let state = params.state;
        let text_size = font_size_u * unit;
        let height = text_size + 4 * unit;
        let mut name_column = Column::new();
        let mut value_column = Column::new().width(Length::Fill);
        for field in state.updatable_fields() {
            name_column = name_column.push(
                Container::new(
                    Text::new(field.name())
                        .size(text_size)
                        .shaping(Shaping::Advanced)
                        .align_x(Horizontal::Left),
                )
                .center_y(height),
            );
            value_column = value_column.push(
                Container::new(field_value_element(state, field, text_size)).center_y(height),
            );
        }
        Container::new(Scrollable::with_direction(
            Row::new()
                .push(name_column)
                .push(Column::new().width(2 * unit))
                .push(value_column)
                // don't overlap with the scrollbar
                .push(Column::new().width(2 * unit)),
            Direction::Vertical(Scrollbar::new().width(unit).scroller_width(unit)),
        ))
        .height(Length::Fill)
        .into()
    }
}

pub struct ToElementCommonParams<'a> {
    pub state: &'a dyn StateExtractor,
    pub window_id: Id,
}

trait ToElementFieldType {
    fn to_element<'a>(
        &'a self,
        field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message>;
}

impl<T> ToElementFieldType for EnumDesc<T>
where
    T: ToString + PartialEq + Clone,
{
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message> {
        if self.is_enabled(state) {
            PickList::new(self.variants(), self.cur_value(state), |selected| {
                self.on_selected(state, selected)
            })
            .text_size(text_size)
            .into()
        } else {
            Text::new(
                self.cur_value(state)
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            )
            .size(text_size)
            .shaping(Shaping::Advanced)
            .into()
        }
    }
}

impl<T> ToElementFieldType for OwnedEnumDesc<T>
where
    T: ToString + PartialEq + Clone,
{
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message> {
        if self.is_enabled(state) {
            PickList::new(self.variants(), self.cur_value(state), |selected| {
                self.on_selected(state, selected)
            })
            .text_size(text_size)
            .into()
        } else {
            Text::new(
                self.cur_value(state)
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            )
            .size(text_size)
            .shaping(Shaping::Advanced)
            .into()
        }
    }
}

impl<T> ToElementFieldType for DynamicEnumDesc<T>
where
    T: ToString + PartialEq + Clone,
{
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message> {
        let (variants, selected) = self.variants_and_selected(state);
        if self.is_enabled(state) {
            PickList::new(variants, selected, |selected| {
                self.on_selected(state, selected)
            })
            .text_size(text_size)
            .into()
        } else {
            Text::new(selected.map(|s| s.to_string()).unwrap_or_default())
                .size(text_size)
                .shaping(Shaping::Advanced)
                .into()
        }
    }
}

impl<T> ToElementFieldType for StepDesc<T>
where
    T: ToString,
{
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message> {
        let cur_value = self.cur_value(state);
        Row::new()
            .align_y(Vertical::Center)
            .spacing(text_size)
            .push(
                Button::new(Text::new("-").size(text_size))
                    .on_press_maybe(self.on_decreased(state)),
            )
            .push(Text::new(cur_value.to_string()).size(text_size))
            .push(
                Button::new(Text::new("+").size(text_size))
                    .on_press_maybe(self.on_increased(state)),
            )
            .into()
    }
}

impl ToElementFieldType for TextDesc {
    fn to_element<'a>(
        &'a self,
        field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: u16,
    ) -> Element<'a, Message> {
        TextInput::new(
            &self.placeholder(field, state).unwrap_or_default(),
            &self.cur_value(field, state).unwrap_or_default(),
        )
        .on_input_maybe(self.on_input_maybe(field, state))
        .on_paste_maybe(self.on_paste_maybe(field, state))
        .on_submit_maybe(self.on_submit_maybe(field, state))
        .width(text_size * 20)
        .size(text_size)
        .into()
    }
}

fn nerd_icon<'a, Message: 'a>(icon: char, size: u16, color: Color) -> Element<'a, Message> {
    Text::new(icon)
        .size(size)
        .font(Font::with_name("fcitx5 osk nerd"))
        .shaping(Shaping::Advanced)
        .color(color)
        .into()
}

fn nerd_btn<'a, Message: 'a>(icon: char, font_size: u16, color: Color) -> Button<'a, Message> {
    Button::new(
        Container::new(nerd_icon(icon, font_size, color))
            .height(Length::Fill)
            .align_y(Vertical::Center),
    )
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
        .shaping(Shaping::Advanced)
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
    let icon = include_bytes!("../../assets/icons/fcitx5-osk.svg");
    let svg = Svg::new(SvgHandle::from_memory(icon)).width(width);
    Button::new(svg)
        .width(width)
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}

fn field_value_element<'a>(
    state: &'a dyn StateExtractor,
    field: &'a Field,
    text_size: u16,
) -> Element<'a, Message> {
    match field.typ() {
        FieldType::StepU16(step_desc) => step_desc.to_element(field, state, text_size),
        FieldType::OwnedEnumPlacement(enum_desc) => enum_desc.to_element(field, state, text_size),
        FieldType::OwnedEnumIndicatorDisplay(enum_desc) => {
            enum_desc.to_element(field, state, text_size)
        }
        FieldType::EnumString(enum_desc) => enum_desc.to_element(field, state, text_size),
        FieldType::DynamicEnumString(enum_desc) => enum_desc.to_element(field, state, text_size),
        FieldType::Text(text_desc) => text_desc.to_element(field, state, text_size),
    }
}
