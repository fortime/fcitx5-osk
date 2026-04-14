//! In this layout, the unit of length is not pixel or meter. It is 1/8 of a normal
//! key's width.

use getset::{CopyGetters, Getters};
use iced::{
    advanced::svg::Handle as SvgHandle,
    alignment::{Horizontal, Vertical},
    padding,
    widget::{
        button::{Style as ButtonStyle, DEFAULT_PADDING},
        container::Style as ContainerStyle,
        scrollable::{Direction, Scrollbar},
        text::Shaping,
        text_input::TextInput,
        Button, Column, Container, PickList, Row, Scrollable, Slider, Space, Svg, Text, Toggler,
    },
    window::Id,
    Color, Element, Font, Length, Padding, Pixels, Size, Theme,
};
use num_traits::FromPrimitive;
use serde::{
    de::{self, Error, Unexpected, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

use std::{
    collections::HashMap,
    fmt::{Display, Formatter, Result as FmtResult},
    ops::{Add, AddAssign, Div, Mul, Sub, SubAssign},
    path::PathBuf,
    result::Result as StdResult,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use crate::{
    app::{KeyboardError, Message},
    config::{IndicatorDisplay, QuickActionBarState},
    dbus::server::ImPanelEvent,
    font,
    state::{
        BoolDesc, CloseOpSource, DynamicEnumDesc, EnumDesc, Field, FieldType, ImEvent,
        KeyboardEvent, LayoutEvent, OwnedEnumDesc, RangeDesc, StateExtractor, StepDesc, StoreEvent,
        TextDesc, UpdateConfigEvent, WindowEvent, WindowManagerEvent,
    },
    store::IdAndConfigPath,
    widget::{self, ExtButton, ExtPickList as _, Movable, Toggle, ToggleCondition, BORDER_RADIUS},
    window::WindowManagerMode,
};

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct KLength(f32);

impl KLength {
    pub fn val(&self) -> f32 {
        self.0
    }
}

impl Default for KLength {
    fn default() -> Self {
        Self(0.)
    }
}

impl Display for KLength {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&format!("k({})", self.0))
    }
}

impl std::fmt::Debug for KLength {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self, f)
    }
}

impl Sub<u32> for KLength {
    type Output = Self;

    fn sub(self, rhs: u32) -> Self::Output {
        Self(self.0 - rhs as f32)
    }
}

impl Mul<u32> for KLength {
    type Output = Self;

    fn mul(self, rhs: u32) -> Self::Output {
        Self(self.0 * rhs as f32)
    }
}

impl Div<u32> for KLength {
    type Output = Self;

    fn div(self, rhs: u32) -> Self::Output {
        Self(self.0 / rhs as f32)
    }
}

impl Mul<KLength> for u32 {
    type Output = KLength;

    fn mul(self, rhs: KLength) -> Self::Output {
        KLength(self as f32 * rhs.0)
    }
}

impl Mul<&u32> for KLength {
    type Output = Self;

    fn mul(self, rhs: &u32) -> Self::Output {
        Self(self.0 * (*rhs) as f32)
    }
}

impl Mul<KLength> for &u32 {
    type Output = KLength;

    fn mul(self, rhs: KLength) -> Self::Output {
        KLength((*self) as f32 * rhs.0)
    }
}

impl Div<f32> for KLength {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl Add for KLength {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        KLength(self.0 + rhs.0)
    }
}

impl Sub for KLength {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        KLength(self.0 - rhs.0)
    }
}

impl Mul for KLength {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        KLength(self.0 * rhs.0)
    }
}

impl Div for KLength {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        KLength(self.0 / rhs.0)
    }
}

impl AddAssign for KLength {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for KLength {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl<'de> Deserialize<'de> for KLength {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KLengthVisitor;

        impl<'de> Visitor<'de> for KLengthVisitor {
            type Value = KLength;

            fn expecting(&self, formatter: &mut Formatter) -> FmtResult {
                formatter.write_str("a u32 or f32")
            }

            fn visit_i64<E>(self, v: i64) -> Result<KLength, E>
            where
                E: de::Error,
            {
                Ok(KLength(v as f32))
            }

            fn visit_f64<E>(self, v: f64) -> Result<KLength, E>
            where
                E: de::Error,
            {
                Ok(KLength(v as f32))
            }
        }

        deserializer.deserialize_f32(KLengthVisitor)
    }
}

impl Serialize for KLength {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f32(self.0)
    }
}

impl From<KLength> for Pixels {
    fn from(value: KLength) -> Self {
        value.0.into()
    }
}

impl From<KLength> for Length {
    fn from(value: KLength) -> Self {
        value.0.into()
    }
}

impl From<f32> for KLength {
    fn from(value: f32) -> Self {
        KLength(value)
    }
}

pub trait FromKLengthSize {
    fn to_iced_size(self) -> Size;
}

impl FromKLengthSize for Size<KLength> {
    fn to_iced_size(self) -> Size {
        Size::new(self.width.0, self.height.0)
    }
}

#[derive(Deserialize, CopyGetters, Getters)]
pub struct KeyAreaLayout {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    /// vertical space between rows
    #[serde(alias = "spacing", default = "KeyAreaLayout::default_spacing_u")]
    spacing_u: u32,
    elements: Vec<KeyRow>,
    #[getset(get = "pub")]
    key_mappings: HashMap<String, KeyId>,
    #[serde(
        alias = "primary_text_size",
        default = "KeyAreaLayout::default_primary_text_size_u"
    )]
    #[getset(get_copy = "pub")]
    primary_text_size_u: u32,
    #[serde(
        alias = "secondary_text_size",
        default = "KeyAreaLayout::default_secondary_text_size_u"
    )]
    #[getset(get_copy = "pub")]
    secondary_text_size_u: u32,
    #[serde(
        alias = "popup_key_width",
        default = "KeyAreaLayout::default_popup_key_width_u"
    )]
    #[getset(get_copy = "pub")]
    popup_key_width_u: u32,
    #[serde(
        alias = "popup_key_height",
        default = "KeyAreaLayout::default_popup_key_height_u"
    )]
    #[getset(get_copy = "pub")]
    popup_key_height_u: u32,
    #[serde(
        alias = "min_toolbar_height",
        default = "KeyAreaLayout::default_min_toolbar_height_u"
    )]
    #[getset(get_copy = "pub")]
    min_toolbar_height_u: u32,
    #[getset(get = "pub")]
    font: Option<String>,
}

impl KeyAreaLayout {
    fn default_spacing_u() -> u32 {
        1
    }

    fn default_primary_text_size_u() -> u32 {
        3
    }

    fn default_popup_key_width_u() -> u32 {
        8
    }

    fn default_popup_key_height_u() -> u32 {
        6
    }

    fn default_secondary_text_size_u() -> u32 {
        2
    }

    fn default_min_toolbar_height_u() -> u32 {
        6
    }

    pub fn width_u(&self) -> u32 {
        self.elements.iter().map(KeyRow::width_u).max().unwrap_or(0)
    }

    pub fn height_u(&self) -> u32 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut height_u = self.spacing_u * (self.elements.len() as u32 - 1);
        height_u += self.elements.iter().map(KeyRow::height_u).sum::<u32>();
        height_u
    }

    pub fn size(&self, unit: KLength) -> (KLength, KLength) {
        (self.width_u() * unit, self.height_u() * unit)
    }

    pub fn to_element<'b>(
        &self,
        unit: KLength,
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
    height_u: u32,
    #[serde(alias = "spacing")]
    /// horizontal space between elements
    spacing_u: u32,
    elements: Vec<KeyRowElement>,
}

impl KeyRow {
    fn width_u(&self) -> u32 {
        if self.elements.is_empty() {
            return 0;
        }
        let mut width_u = self.spacing_u * (self.elements.len() as u32 - 1);
        width_u += self
            .elements
            .iter()
            .map(KeyRowElement::width_u)
            .sum::<u32>();
        width_u
    }

    fn height_u(&self) -> u32 {
        self.height_u
    }

    fn to_element<'b>(
        &self,
        unit: KLength,
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
    Padding(u32),
    Key {
        width_u: u32,
        height_u: Option<u32>,
        name: Arc<str>,
    },
}

impl KeyRowElement {
    fn width_u(&self) -> u32 {
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
        max_height_u: u32,
        unit: KLength,
        state: &'b dyn StateExtractor,
    ) -> Element<'b, Message> {
        match self {
            KeyRowElement::Padding(width_u) => Space::new().width(width_u * unit).into(),
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
                    &"it should end with an empty string or a u32 integer",
                )),
            }
        } else if typ.starts_with("k") {
            let width_u = items.next().unwrap_or("8");
            let width_u = match width_u.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(Error::invalid_value(
                        Unexpected::Str(width_u),
                        &"width should be empty or a u32 integer",
                    ))
                }
            };
            let height_u = items
                .next()
                .map(|s| {
                    s.parse().map_err(|_| {
                        Error::invalid_value(
                            Unexpected::Str(s),
                            &"height should be empty or a u32 integer",
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
    height_u: u32,
    quick_action_bar_state: QuickActionBarState,
    quick_action_bar_shown: bool,
    repeat_action_shown: bool,
}

impl ToolbarLayout {
    pub fn new(min_toolbar_height_u: u32, quick_action_bar_state: QuickActionBarState) -> Self {
        let mut res = Self {
            height_u: 1,
            quick_action_bar_state,
            quick_action_bar_shown: false,
            repeat_action_shown: false,
        };
        // make sure height_u is valid
        res.update_height_u(min_toolbar_height_u);
        res.update_quick_action_bar_state(quick_action_bar_state);
        res
    }

    pub fn update_height_u(&mut self, min_toolbar_height_u: u32) {
        self.height_u = 1.max(min_toolbar_height_u)
    }

    pub fn update_quick_action_bar_state(
        &mut self,
        quick_action_bar_state: QuickActionBarState,
    ) -> bool {
        let shown = self.quick_action_bar_shown;
        self.quick_action_bar_state = quick_action_bar_state;
        self.quick_action_bar_shown = quick_action_bar_state == QuickActionBarState::On;
        self.quick_action_bar_shown != shown
    }

    pub fn toggle_quick_action_bar(&mut self) -> bool {
        if self.quick_action_bar_state == QuickActionBarState::Toggle {
            self.quick_action_bar_shown = !self.quick_action_bar_shown;
            true
        } else {
            false
        }
    }

    pub fn toggle_repeat_action(&mut self) {
        self.repeat_action_shown = !self.repeat_action_shown;
    }

    pub fn height_u(&self) -> u32 {
        if self.quick_action_bar_shown {
            // toolbar(height_u) + quick action bar(height_u + 2)
            self.height_u * 2 + 2
        } else {
            self.height_u
        }
    }

    pub fn to_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        candidate_font: Font,
        font_size_u: u32,
    ) -> Element<'b, Message> {
        let height = self.height_u * unit;
        let font_size = font_size_u * unit;
        let mut column = Column::new();
        let base = if params.state.im().candidate_area_state().has_candidate() {
            self.candidate_element(params, unit, candidate_font, font_size)
        } else {
            self.toolbar_element(params, unit, font_size)
        };
        column = column.push(
            Container::new(base)
                .height(height)
                // Add the padding of the keyboard
                .padding(padding::horizontal(unit)),
        );
        if self.quick_action_bar_shown {
            column = column.push(
                Container::new(self.quick_action_bar_element(params, unit, font_size))
                    .height(height + 2 * unit)
                    .style(|theme: &Theme| ContainerStyle {
                        background: Some(theme.extended_palette().background.weak.color.into()),
                        ..Default::default()
                    }),
            );
        }
        column.into()
    }

    fn candidate_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font: Font,
        font_size: KLength,
    ) -> Element<'b, Message> {
        let theme = params.state.theme();
        let state = params.state.im().candidate_area_state();
        let spacing = 2 * unit;
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
                    .map(|c| c.iter().map(|t| t.0.chars().count()).sum())
                    .max()
                    .unwrap_or(0) as u32
                    * char_width,
            )
        } else {
            let mut consumed = 0;
            let mut max_width = 0;
            for candidate in candidate_list {
                // TODO Simply assume one char consumes 1 * font_size. Calculate the width in the
                // future.
                let width = candidate.iter().map(|t| t.0.chars().count()).sum::<usize>() as u32;
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
        let (consumed, max_width) = if consumed <= 1 {
            // as least 1
            (1, Length::Fill)
        } else {
            (consumed, Length::Fixed(max_width.val()))
        };
        let mut index = state.cursor();
        for candidate in &candidate_list[..consumed] {
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
        let candidate_element: Element<_> = if state.is_paged() || consumed == 1 {
            Scrollable::with_direction(
                candidate_row,
                Direction::Horizontal(Scrollbar::new().width(1).spacing(unit)),
            )
            .style(widget::scrollable_style)
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
                unit,
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
                unit,
            )
            .on_press_maybe(next_message),
        );

        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .into()
    }

    fn toolbar_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font_size: KLength,
    ) -> Element<'b, Message> {
        let state = params.state;
        let theme = state.theme();
        let color = theme.extended_palette().background.weak.text;
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
                    Some(ImPanelEvent::NewVisibleRequest(false).into())
                } else {
                    Some(WindowManagerEvent::CloseKeyboard(CloseOpSource::UserAction).into())
                }
            }
        };
        if let Some(message) = indicator_message {
            row = row.push(nerd_btn('󰁄', font_size, color, unit).on_press(message));
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
                    .all_size(font_size),
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
                    .all_size(font_size),
                ),
        );

        let mut tray = Row::new();
        if self.quick_action_bar_state == QuickActionBarState::Toggle {
            let shown = self.quick_action_bar_shown;
            tray = tray.push(
                Container::new(
                    nerd_btn('', font_size, color, unit)
                        .on_press(LayoutEvent::ToggleQuickActionBar.into()),
                )
                .align_x(Horizontal::Center)
                .style(move |theme: &Theme| {
                    let mut style = ContainerStyle::default();
                    if shown {
                        style.background =
                            Some(theme.extended_palette().background.weak.color.into());
                    }
                    style
                }),
            );
        }
        tray = tray.push(
            nerd_btn('󰘮', font_size, color, unit).on_press(LayoutEvent::ToggleSetting.into()),
        );
        row = row.push(tray);
        Container::new(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Right)
            .into()
    }

    fn quick_action_bar_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font_size: KLength,
    ) -> Element<'b, Message> {
        let mut row = Row::new()
            .height(Length::Fill)
            .spacing(unit * 2)
            .padding(Padding::new(unit.val()))
            .align_y(Vertical::Center);
        row = row.push(
            ExtButton::new(Text::new("Reload").size(font_size))
                .border_radius(BORDER_RADIUS)
                .padding(DEFAULT_PADDING)
                .on_release_with(Some(|| StoreEvent::Load.into())),
        );
        row = row.push(self.combo_action_element(params, unit, font_size));
        row = row.push(self.repeat_action_element(params, unit, font_size));
        Container::new(
            Scrollable::with_direction(
                row,
                Direction::Horizontal(Scrollbar::new().width(1).spacing(unit)),
            )
            .style(widget::scrollable_style),
        )
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    fn combo_action_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font_size: KLength,
    ) -> Element<'b, Message> {
        let font = params.state.keyboard().default_font();
        let is_combo_mode = params.state.keyboard().is_combo_mode();
        let content = Row::new()
            .align_y(Vertical::Center)
            .push(
                Text::new("Combo")
                    .size(font_size)
                    .shaping(Shaping::Advanced),
            )
            .spacing(unit)
            .push(
                Toggler::new(is_combo_mode)
                    .size(font_size)
                    .style(widget::toggler_style)
                    .on_toggle(|_| KeyboardEvent::ToggleComboMode.into()),
            );
        if is_combo_mode {
            Row::new()
                .spacing(unit)
                .push(widget::button_container(content))
                .push(
                    ExtButton::new(
                        Text::new("Release")
                            .size(font_size)
                            .font(font)
                            .shaping(Shaping::Advanced),
                    )
                    .border_radius(BORDER_RADIUS)
                    .padding(DEFAULT_PADDING)
                    .on_release_with(Some(|| KeyboardEvent::InsertComboKeyRelease.into())),
                )
                .push(
                    ExtButton::new(
                        Text::new("ReleaseAll")
                            .size(font_size)
                            .font(font)
                            .shaping(Shaping::Advanced),
                    )
                    .border_radius(BORDER_RADIUS)
                    .padding(DEFAULT_PADDING)
                    .on_release_with(Some(|| KeyboardEvent::InsertComboKeyReleaseAll.into())),
                )
                .into()
        } else {
            widget::button_container(content).into()
        }
    }

    fn repeat_action_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font_size: KLength,
    ) -> Element<'b, Message> {
        let font = params.state.keyboard().default_font();
        let content = Row::new()
            .align_y(Vertical::Center)
            .push(
                Text::new("Repeat")
                    .size(font_size)
                    .shaping(Shaping::Advanced),
            )
            .spacing(unit)
            .push(
                Toggler::new(self.repeat_action_shown)
                    .size(font_size)
                    .style(widget::toggler_style)
                    .on_toggle(|_| LayoutEvent::ToggleRepeatAction.into()),
            );
        let repeat_keys = params.state.keyboard().repeat_keys();
        if self.repeat_action_shown && !repeat_keys.is_empty() {
            let repeat_serial = params.state.keyboard().repeat_serial();
            let mut texts = Row::new()
                .padding(padding::horizontal(1.5 * font_size.val()))
                .push(
                    Text::new(repeat_keys[0].to_string())
                        .font(repeat_keys[0].font().unwrap_or(font))
                        .shaping(Shaping::Advanced)
                        .size(font_size),
                );
            for key in repeat_keys.iter().skip(1) {
                texts = texts.push(
                    Text::new(" + ".to_string())
                        .font(font)
                        .shaping(Shaping::Advanced)
                        .size(font_size),
                );
                texts = texts.push(
                    Text::new(key.to_string())
                        .font(key.font().unwrap_or(font))
                        .shaping(Shaping::Advanced)
                        .size(font_size),
                );
            }
            Row::new()
                .spacing(unit)
                .push(widget::button_container(content))
                .push(
                    ExtButton::new(texts)
                        .border_radius(BORDER_RADIUS)
                        .padding(DEFAULT_PADDING)
                        .on_release_with(Some(|| KeyboardEvent::StopRepeating.into()))
                        .on_press_with(Some(move || {
                            KeyboardEvent::RepeatComboKeys((
                                repeat_serial,
                                Duration::from_millis(500),
                            ))
                            .into()
                        })),
                )
                .into()
        } else {
            widget::button_container(content).into()
        }
    }
}

pub struct SettingLayout;

impl SettingLayout {
    pub fn to_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
        unit: KLength,
        font_size_u: u32,
    ) -> Element<'b, Message> {
        let state = params.state;
        let text_size = font_size_u * unit;
        let height = text_size + 4 * unit;
        let mut name_column = Column::new();
        let mut value_column = Column::new().width(Length::Fill);
        for field in state.updatable_fields() {
            let row_num = field_row_num(state, field);
            name_column = name_column.push(
                Container::new(
                    Text::new(field.name())
                        .size(text_size)
                        .shaping(Shaping::Advanced)
                        .align_x(Horizontal::Left),
                )
                .center_y(height * row_num),
            );
            value_column = value_column.push(
                Container::new(field_value_element(state, field, text_size))
                    .center_y(height * row_num),
            );
        }
        Container::new(
            Scrollable::with_direction(
                Row::new()
                    .push(name_column)
                    .push(Column::new().width(2 * unit))
                    .push(value_column)
                    // don't overlap with the scrollbar
                    .push(Column::new().width(2 * unit)),
                Direction::Vertical(Scrollbar::new().width(unit).scroller_width(unit)),
            )
            .style(widget::scrollable_style),
        )
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
        text_size: KLength,
    ) -> Element<'a, Message>;

    fn row_num(&self, _state: &dyn StateExtractor) -> u32 {
        1
    }
}

impl<T> ToElementFieldType for EnumDesc<T>
where
    T: ToString + PartialEq + Clone,
{
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: KLength,
    ) -> Element<'a, Message> {
        if self.is_enabled(state) {
            PickList::new(self.variants(), self.cur_value(state), |selected| {
                self.on_selected(state, selected)
            })
            .all_size(text_size)
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
        text_size: KLength,
    ) -> Element<'a, Message> {
        if self.is_enabled(state) {
            PickList::new(self.variants(), self.cur_value(state), |selected| {
                self.on_selected(state, selected)
            })
            .all_size(text_size)
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
        text_size: KLength,
    ) -> Element<'a, Message> {
        let (variants, selected) = self.variants_and_selected(state);
        if self.is_enabled(state) {
            PickList::new(variants, selected, |selected| {
                self.on_selected(state, selected)
            })
            .all_size(text_size)
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
        text_size: KLength,
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

impl<T> ToElementFieldType for RangeDesc<T>
where
    T: 'static + Copy + From<u8> + PartialOrd + Into<f64> + FromPrimitive + Display + FromStr,
{
    fn to_element<'a>(
        &'a self,
        field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: KLength,
    ) -> Element<'a, Message> {
        let cur_value = self.cur_value(field, state);
        let padding = DEFAULT_PADDING;
        let mut column = Column::new().push(
            Container::new(
                Text::new(self.format(state, cur_value))
                    .size(text_size)
                    .shaping(Shaping::Advanced),
            )
            .align_y(Vertical::Center)
            .align_x(Horizontal::Center)
            .padding(padding)
            .style(|theme: &Theme| ContainerStyle {
                background: Some(theme.extended_palette().background.weak.color.into()),
                ..Default::default()
            }),
        );
        if self.is_enabled(state) {
            let init_value = self.init_value(state);
            let min_value = self.min_value(state);
            let max_value = self.max_value(state);
            let key = field.id();
            let slider = Slider::new(min_value..=max_value, cur_value, move |v| {
                if self.check(state, v) {
                    Message::from(UpdateConfigEvent::ChangeTempText {
                        key: key.to_string(),
                        init_value: init_value.to_string(),
                        value: v.to_string(),
                    })
                } else {
                    KeyboardError::Error(Arc::new(anyhow::anyhow!(
                        r#"Invalid value for option["{}"]: {}"#,
                        key,
                        v
                    )))
                    .into()
                }
            })
            .height(text_size)
            .style(widget::slider_style_cb(text_size))
            .on_release(Message::from(UpdateConfigEvent::SubmitTempText {
                key: key.to_string(),
                init_value: init_value.to_string(),
                producer: self.on_changed_cb(),
            }));
            let mut slider_row = Row::new().spacing(text_size / 2).align_y(Vertical::Center);
            slider_row = slider_row.push(
                Text::new(self.format(state, min_value))
                    .size(text_size / 2)
                    .shaping(Shaping::Advanced),
            );
            slider_row = slider_row.push(slider);
            slider_row = slider_row.push(
                Text::new(self.format(state, max_value))
                    .size(text_size / 2)
                    .shaping(Shaping::Advanced),
            );
            column = column.push(
                Container::new(slider_row)
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Center)
                    .padding(padding),
            );
        }
        column.into()
    }

    fn row_num(&self, state: &dyn StateExtractor) -> u32 {
        if self.is_enabled(state) {
            2
        } else {
            1
        }
    }
}

impl ToElementFieldType for TextDesc {
    fn to_element<'a>(
        &'a self,
        field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: KLength,
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

impl ToElementFieldType for BoolDesc {
    fn to_element<'a>(
        &'a self,
        _field: &'a Field,
        state: &'a dyn StateExtractor,
        text_size: KLength,
    ) -> Element<'a, Message> {
        let cur_value = self.cur_value(state);
        let mut toggler = Toggler::new(cur_value)
            .size(text_size)
            .style(widget::toggler_style);
        if self.is_enabled(state) {
            toggler = toggler.on_toggle(|value| self.on_changed(state, value))
        }
        toggler.into()
    }
}

fn nerd_icon<'a, Message: 'a>(icon: char, size: KLength, color: Color) -> Element<'a, Message> {
    Text::new(icon)
        .size(size)
        .center()
        .font(font::load("fcitx5 osk nerd"))
        .shaping(Shaping::Advanced)
        .color(color)
        .into()
}

fn nerd_btn<'a, Message: 'a>(
    icon: char,
    font_size: KLength,
    color: Color,
    unit: KLength,
) -> Button<'a, Message> {
    Button::new(
        Container::new(nerd_icon(icon, font_size, color))
            .height(Length::Fill)
            .center_x(font_size + 2 * unit)
            .align_y(Vertical::Center),
    )
    .width(font_size + 2 * unit)
    .height(Length::Fill)
    .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
    .padding(0)
}

fn candidate_btn(
    candidate: &Vec<(String, Option<Font>)>,
    font: Font,
    font_size: KLength,
    width: Length,
) -> Button<'_, Message> {
    let mut texts = Row::new();
    for chars in candidate {
        texts = texts.push(
            Text::new(&chars.0)
                .font(chars.1.unwrap_or(font))
                .shaping(Shaping::Advanced)
                .size(font_size)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center),
        );
    }
    Button::new(Container::new(texts).center(Length::Fill))
        .width(width)
        .style(|_, _| ButtonStyle::default().with_background(Color::TRANSPARENT))
        .padding(0)
}

pub fn indicator_btn<'a, Message>(width: KLength) -> Button<'a, Message>
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
    text_size: KLength,
) -> Element<'a, Message> {
    match field.typ() {
        FieldType::StepU32(desc) => desc.to_element(field, state, text_size),
        FieldType::RangeF32(desc) => desc.to_element(field, state, text_size),
        FieldType::OwnedEnumPlacement(desc) => desc.to_element(field, state, text_size),
        FieldType::OwnedEnumIndicatorDisplay(desc) => desc.to_element(field, state, text_size),
        FieldType::OwnedEnumQuickActionBarState(desc) => desc.to_element(field, state, text_size),
        FieldType::EnumString(desc) => desc.to_element(field, state, text_size),
        FieldType::DynamicEnumString(desc) => desc.to_element(field, state, text_size),
        FieldType::Text(desc) => desc.to_element(field, state, text_size),
        FieldType::Bool(desc) => desc.to_element(field, state, text_size),
    }
}

fn field_row_num<'a>(state: &'a dyn StateExtractor, field: &'a Field) -> u32 {
    match field.typ() {
        FieldType::StepU32(desc) => desc.row_num(state),
        FieldType::RangeF32(desc) => desc.row_num(state),
        FieldType::OwnedEnumPlacement(desc) => desc.row_num(state),
        FieldType::OwnedEnumIndicatorDisplay(desc) => desc.row_num(state),
        FieldType::OwnedEnumQuickActionBarState(desc) => desc.row_num(state),
        FieldType::EnumString(desc) => desc.row_num(state),
        FieldType::DynamicEnumString(desc) => desc.row_num(state),
        FieldType::Text(desc) => desc.row_num(state),
        FieldType::Bool(desc) => desc.row_num(state),
    }
}
