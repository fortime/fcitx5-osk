use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{Display, Formatter as FmtFormatter, Result as FmtResult},
    mem,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use getset::Getters;
use iced::{
    alignment::{Horizontal, Vertical},
    futures::lock::Mutex as IcedFuturesMutex,
    widget::{container::Style as ContainerStyle, text::Shaping, Column, Container, Row, Text},
    Element, Font, Padding, Task,
};
use tokio::time;
use xkeysym::Keysym;
use zbus::{Connection, Result as ZbusResult};

use crate::{
    app::{self, Message},
    dbus::{
        client::{
            Fcitx5ControllerServiceProxy, Fcitx5VirtualKeyboardBackendServiceProxy,
            Fcitx5VirtualKeyboardServiceProxy, IFcitx5ControllerService,
            IFcitx5VirtualKeyboardBackendService, IFcitx5VirtualKeyboardService,
            WorkaroundFcitx5VirtualKeyboardBackendService,
        },
        server::{CandidateAreaState, Fcitx5VirtualkeyboardImPanelEvent},
    },
    font,
    key_set::{Key, KeyValue},
    layout::{KLength, KeyAreaLayout},
    state::ImEvent,
    store::Store,
    widget::{Key as KeyWidget, KeyEvent as KeyWidgetEvent, PopupKey, BORDER_RADIUS},
};

const TEXT_PADDING_LENGTH: u32 = 3;

/// #define KEY_LEFTSHIFT       42, val + 8
const KEYCODE_LEFT_SHIFT: i16 = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ModifierState {
    NoState = 0x0,
    Shift = 1 << 0,
    CapsLock = 1 << 1,
    Ctrl = 1 << 2,
    Alt = 1 << 3,
    NumLock = 1 << 4,
    Super = 1 << 6,
    // Virtual = 1 << 29,
    // Repeat = 1 << 31,
}

impl ModifierState {
    pub fn is_set(&self, state: u32) -> bool {
        *self as u32 & state != 0
    }
}

struct KeyState {
    pressed_time: u128,
    selected_key_value: KeyValue,
}

struct HoldingKeyState {
    name: Arc<str>,
    key_widget_event: KeyWidgetEvent,
    key: Key,
    // The order of SelectSecondary/UnselectSecondary is uncertain when moving between popup keys,
    // we use this flags to record which key is selected, once it is empty, we will use the primary
    // keysym.
    flags: Vec<KeyValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fcitx5Hidden {
    Unset,
    Clearing,
    Set,
}

pub struct KeyboardState {
    id: u8,
    primary_text_size_u: u32,
    secondary_text_size_u: u32,
    font: Font,
    keys: HashMap<String, Key>,
    pressed_keys: HashMap<Arc<str>, KeyState>,
    view_flags: u32,
    holding_timeout: Duration,
    holding_key_state: Option<HoldingKeyState>,
    popup_key_width_u: u32,
    popup_key_height_u: u32,
    /// if there is no indicator and fcitx5 hides virtual keyboard, we won't hide the keyboard,
    /// instead we set this flag, and tell fcitx5 to show virtual keyboard when there is any key
    /// pressing event.
    fcitx5_hidden: Fcitx5Hidden,
    keyboard_backend: KeyboardBackend,
    keyboard_backend_state: KeyboardBackendState,
}

impl KeyboardState {
    pub fn new(
        holding_timeout: Duration,
        key_area_layout: &KeyAreaLayout,
        store: &Store,
        keyboard_backend: KeyboardBackend,
    ) -> Self {
        let mut res = Self {
            id: 0,
            primary_text_size_u: Default::default(),
            secondary_text_size_u: Default::default(),
            font: Default::default(),
            keys: HashMap::new(),
            pressed_keys: HashMap::new(),
            view_flags: 0,
            holding_timeout,
            holding_key_state: None,
            popup_key_width_u: 0,
            popup_key_height_u: 0,
            fcitx5_hidden: Fcitx5Hidden::Unset,
            keyboard_backend,
            keyboard_backend_state: Default::default(),
        };
        res.update_key_area_layout(key_area_layout, store);
        res
    }

    pub fn update_key_area_layout(&mut self, key_area_layout: &KeyAreaLayout, store: &Store) {
        self.id = self.id.wrapping_add(1);
        self.primary_text_size_u = key_area_layout.primary_text_size_u();
        self.secondary_text_size_u = key_area_layout.secondary_text_size_u();
        self.popup_key_width_u = key_area_layout.popup_key_width_u();
        self.popup_key_height_u = key_area_layout.popup_key_height_u();
        self.keys = key_area_layout
            .key_mappings()
            .iter()
            .filter_map(|(k, v)| store.key(v).map(|key| (k.clone(), key.clone())))
            .collect();
        self.pressed_keys.clear();
        self.holding_key_state = None;
        self.font = key_area_layout
            .font()
            .as_ref()
            .map(|n| font::load(n))
            .unwrap_or_default();
        self.keyboard_backend_state = Default::default();
    }

    pub fn select_candidate(&mut self, cursor: usize) -> Task<Message> {
        self.clear_fcitx5_hidden().chain(
            self.keyboard_backend
                .select_candidate(&mut self.keyboard_backend_state, cursor),
        )
    }

    pub fn on_event(&mut self, event: KeyboardEvent) -> Task<Message> {
        match event {
            KeyboardEvent::UnsetFcitx5Hidden => {
                if self.fcitx5_hidden == Fcitx5Hidden::Clearing {
                    self.fcitx5_hidden = Fcitx5Hidden::Unset;
                }
                Message::nothing()
            }
            KeyboardEvent::ToggleComboMode => {
                if self.pressed_keys.is_empty() {
                    // make sure virtual keyboard is enabled in fcitx5
                    self.clear_fcitx5_hidden().chain(
                        self.keyboard_backend
                            .toggle_combo_mode(&mut self.keyboard_backend_state),
                    )
                } else {
                    // do nothing if there is any keys pressed
                    Message::nothing()
                }
            }
            KeyboardEvent::InsertComboKeyRelease => self.keyboard_backend.insert_combo_key(
                &mut self.keyboard_backend_state,
                ComboKey::Release,
                self.font,
            ),
            KeyboardEvent::InsertComboKeyReleaseAll => self.keyboard_backend.insert_combo_key(
                &mut self.keyboard_backend_state,
                ComboKey::ReleaseAll,
                self.font,
            ),
            KeyboardEvent::RepeatComboKeys((serial, timeout)) => {
                tracing::debug!("repeat combo keys: {serial}, cur: {}", self.repeat_serial());
                if serial == self.keyboard_backend_state.repeat_serial {
                    self.clear_fcitx5_hidden()
                        .chain(
                            self.keyboard_backend
                                .process_combo_keys(self.keyboard_backend_state.repeat_keys.clone())
                                .map(|r| match r {
                                    Ok(_) => Message::Nothing,
                                    Err(e) => {
                                        app::error_with_context(e, "error to process repeat keys")
                                    }
                                }),
                        )
                        .chain(Task::future(async move {
                            time::sleep(timeout).await;
                            let mut timeout = timeout.div_f32(2.);
                            if timeout.as_millis() < 20 {
                                timeout = Duration::from_millis(20)
                            }
                            KeyboardEvent::RepeatComboKeys((serial, timeout)).into()
                        }))
                } else {
                    Message::nothing()
                }
            }
            KeyboardEvent::StopRepeating => {
                tracing::debug!("stop Repeating combo keys");
                self.keyboard_backend_state.repeat_serial =
                    self.keyboard_backend_state.repeat_serial.wrapping_add(1);
                Message::nothing()
            }
        }
    }

    pub fn on_key_event(&mut self, event: KeyEvent) -> Task<Message> {
        if event.common.id != self.id {
            tracing::debug!(
                "receive event of keyboard state id: {}, expected: {}",
                event.common.id,
                self.id
            );
            return Message::nothing();
        }
        let KeyEvent { common, inner } = event;
        match inner {
            KeyEventInner::Pressed(key_widget_event) => {
                return self.press_key(common, key_widget_event)
            }
            KeyEventInner::Holding(key_widget_event, pressed_time) => {
                self.hold_key(common, key_widget_event, pressed_time);
            }
            KeyEventInner::Released(key_widget_event) => {
                return self.release_key(common, key_widget_event)
            }
            KeyEventInner::SelectSecondary => {
                self.change_selected_secondary(common, true);
            }
            KeyEventInner::UnselectSecondary => {
                self.change_selected_secondary(common, false);
            }
        }
        Message::nothing()
    }

    fn new_popup_key<'a>(
        &'a self,
        holding_key_state: &'a HoldingKeyState,
        key_value: &'a KeyValue,
        unit: KLength,
    ) -> PopupKey<'a, Message> {
        let common =
            KeyEventCommon::new(self.id, holding_key_state.name.clone(), key_value.clone());
        PopupKey::new(
            Text::new(key_value.symbol())
                .shaping(Shaping::Advanced)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .font(key_value.font().unwrap_or(self.font))
                .size(self.primary_text_size_u * unit),
            holding_key_state.key_widget_event.finger,
        )
        .width(self.popup_key_width_u * unit)
        .height(self.popup_key_height_u * unit)
        .border_radius(BORDER_RADIUS)
        .on_enter(KeyEvent::new(common.clone(), KeyEventInner::SelectSecondary).into())
        .on_exit(KeyEvent::new(common, KeyEventInner::UnselectSecondary).into())
    }

    pub fn set_fcitx5_hidden(&mut self) {
        self.fcitx5_hidden = Fcitx5Hidden::Set;
    }

    #[tracing::instrument(skip(self))]
    fn change_selected_secondary(&mut self, common: KeyEventCommon, is_select: bool) {
        let Some(holding_key_state) = &mut self.holding_key_state else {
            tracing::warn!("there is no holding key");
            return;
        };
        if holding_key_state.name != common.key_name {
            tracing::warn!(
                "holding key is not the same, expected: {}",
                holding_key_state.name,
            );
            return;
        }

        if let Some(key_state) = self.pressed_keys.get_mut(&common.key_name) {
            let key_value = common.key_value;
            if is_select {
                key_state.selected_key_value = key_value.clone();
                holding_key_state.flags.push(key_value.clone());
            } else {
                let mut start = 0;
                let mut end = holding_key_state.flags.len();
                while end > start {
                    if holding_key_state.flags[start] == key_value {
                        end -= 1;
                        holding_key_state.flags.swap(start, end);
                    } else {
                        start += 1;
                    }
                }
                holding_key_state.flags.truncate(end);
                if key_state.selected_key_value == key_value {
                    if let Some(key_value) = holding_key_state.flags.last() {
                        key_state.selected_key_value = key_value.clone();
                    } else {
                        // set key_value to the primary one.
                        let is_shift_set = ModifierState::Shift.is_set(self.view_flags);
                        let is_caps_lock_set = ModifierState::CapsLock.is_set(self.view_flags);
                        key_state.selected_key_value = holding_key_state
                            .key
                            .key_value(is_shift_set, is_caps_lock_set);
                    }
                }
            }
        } else {
            tracing::warn!("not pressed: {}", common.key_name);
        }
    }

    pub fn key(
        &self,
        key_name: Arc<str>,
        unit: KLength,
        size: (KLength, KLength),
    ) -> Element<'_, Message> {
        let (width, height) = size;
        let (inner_width, inner_height) = (
            width - TEXT_PADDING_LENGTH * 2,
            height - TEXT_PADDING_LENGTH * 2,
        );

        let (content, press_cb, release_cb) = if let Some(key) = self.keys.get(&*key_name) {
            let is_shift_set = ModifierState::Shift.is_set(self.view_flags);
            let is_caps_lock_set = ModifierState::CapsLock.is_set(self.view_flags);
            let secondary_height = inner_height / 3;
            let primary_height = inner_height - secondary_height;
            // It's related to the conversion between float and int, if we don't minus 1, it may
            // be too large in float, and the text can't be shown
            let secondary_text_size = (secondary_height - 1)
                .val()
                .min((self.secondary_text_size_u * unit).val());
            let primary_text_size = (primary_height - 1)
                .val()
                .min((self.primary_text_size_u * unit).val());
            let mut column: Column<Message> = Column::new();
            let primary_key_value = key.primary();
            let secondary_key_values = key.secondaries();
            let (primary, secondary) = if is_shift_set ^ is_caps_lock_set {
                (
                    secondary_key_values.first().unwrap_or(primary_key_value),
                    secondary_key_values.first().map(|_| primary_key_value),
                )
            } else {
                (primary_key_value, secondary_key_values.first())
            };
            let middle = Text::new(primary.symbol())
                .shaping(Shaping::Advanced)
                .font(primary.font().unwrap_or(self.font));
            let mut top = Row::new().spacing(unit);
            let mut has_secondary = false;
            for secondary in secondary
                .into_iter()
                .chain(secondary_key_values.iter().skip(1))
            {
                has_secondary = true;
                let padding = Text::new(" ").size(TEXT_PADDING_LENGTH as f32);
                let text = Text::new(secondary.symbol())
                    .font(secondary.font().unwrap_or(self.font))
                    .shaping(Shaping::Advanced)
                    .width(inner_width)
                    .height(secondary_height)
                    .size(secondary_text_size)
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Right);
                top = top.push(padding).push(text);
            }
            let key_value = key.key_value(is_shift_set, is_caps_lock_set);
            if has_secondary {
                column = column.push(top.height(secondary_height)).push(
                    middle
                        .width(inner_width)
                        .height(primary_height)
                        .size(primary_text_size)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            } else {
                // If there is no secondary, set it in the middle of the key
                column = column.push(
                    middle
                        .width(inner_width)
                        .height(inner_height)
                        .size(primary_text_size)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            }
            let id = self.id;
            let common = KeyEventCommon::new(id, key_name, key_value);
            (
                Element::from(column),
                Some({
                    let common = common.clone();
                    move |key_widget_event| {
                        Message::from(KeyEvent::new(
                            common.clone(),
                            KeyEventInner::Pressed(key_widget_event),
                        ))
                    }
                }),
                Some(move |key_widget_event| {
                    Message::from(KeyEvent::new(
                        common.clone(),
                        KeyEventInner::Released(key_widget_event),
                    ))
                }),
            )
        } else {
            tracing::debug!("{key_name} is not found");
            (Element::from(Text::new("")), None, None)
        };
        KeyWidget::new(content)
            .on_press_with(press_cb)
            .on_release_with(release_cb)
            .border_radius(BORDER_RADIUS)
            .padding(Padding::new(TEXT_PADDING_LENGTH as f32))
            .width(width)
            .height(height)
            .into()
    }

    pub fn popup_overlay(
        &self,
        unit: KLength,
        size: (KLength, KLength),
    ) -> Option<Element<'_, Message>> {
        const MARGIN_U: u32 = 1;
        let (width, height) = size;

        let holding_key_state = self.holding_key_state.as_ref()?;

        let is_shift_set = ModifierState::Shift.is_set(self.view_flags);
        let is_caps_lock_set = ModifierState::CapsLock.is_set(self.view_flags);

        let key = &holding_key_state.key;
        let mut row = Row::new();
        let mut skip = 0;
        let mut popup_key_area_width = KLength::default();
        if Key::is_shifted(is_shift_set, is_caps_lock_set) {
            row = row.push(self.new_popup_key(holding_key_state, key.primary(), unit));
            skip = 1;
            popup_key_area_width += self.popup_key_width_u * unit;
        }
        for secondary in key.secondaries().iter().skip(skip) {
            row = row.push(self.new_popup_key(holding_key_state, secondary, unit));
            popup_key_area_width += self.popup_key_width_u * unit;
        }

        // calculate position.
        let bounds = &holding_key_state.key_widget_event.bounds;
        let mut left_x = bounds.x;
        if left_x + popup_key_area_width.val() > width.val() {
            left_x = width.val() - popup_key_area_width.val();
            if left_x < 0. {
                left_x = 0.;
            }
        }
        let mut top_y = bounds.y;
        if top_y > (self.popup_key_height_u * unit + MARGIN_U * unit).val() {
            top_y -= (self.popup_key_height_u * unit + MARGIN_U * unit).val();
        } else {
            top_y += bounds.height + (MARGIN_U * unit).val();
        }

        // calculate padding.
        let padding = Padding::default().left(left_x).top(top_y);
        Some(
            Container::new(Container::new(row).style(|theme| {
                let mut style = ContainerStyle::default();
                style.shadow.offset = [1.0, 1.0].into();
                style.shadow.color = theme.extended_palette().background.weak.color;
                style.shadow.blur_radius = 5.;
                style.border = style.border.rounded(5);
                style.background = Some(theme.extended_palette().primary.weak.color.into());
                style
            }))
            .padding(padding)
            .width(width)
            .height(height)
            .into(),
        )
    }
}

// call fcitx5
impl KeyboardState {
    pub(super) fn update_keyboard_backend(&mut self, keyboard_backend: KeyboardBackend) {
        self.keyboard_backend = keyboard_backend;
    }

    fn press_key(
        &mut self,
        common: KeyEventCommon,
        key_widget_event: KeyWidgetEvent,
    ) -> Task<Message> {
        let modifier_state = to_modifier_state(&common.key_value);
        if modifier_state != ModifierState::CapsLock {
            self.view_flags |= modifier_state as u32;
        }

        let key_state = self.pressed_keys.entry(common.key_name.clone());
        let mut contains = true;
        let pressed_time = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);
        {
            let contains = &mut contains;
            key_state.or_insert_with(|| {
                *contains = false;
                KeyState {
                    pressed_time,
                    selected_key_value: common.key_value.clone(),
                }
            });
        }
        let mut task = self.clear_fcitx5_hidden();
        if modifier_state == ModifierState::NoState && !contains {
            let holding_timeout = self.holding_timeout;
            let next = Task::future(async move {
                time::sleep(holding_timeout).await;
                KeyEvent::new(
                    common,
                    KeyEventInner::Holding(key_widget_event, pressed_time),
                )
                .into()
            });
            task = task.chain(next);
        } else if modifier_state != ModifierState::CapsLock
            && modifier_state != ModifierState::Shift
            && !contains
        {
            let key_name = common.key_name.clone();
            let next = self
                .keyboard_backend
                .process_key_events(
                    &mut self.keyboard_backend_state,
                    vec![ProcessKeyEventRequest {
                        key_value: common.key_value,
                        is_release: false,
                        time: pressed_time,
                    }],
                    self.font,
                )
                .map(move |r| {
                    r.unwrap_or_else(|e| {
                        app::error_with_context(
                            e,
                            format!("send key pressed event failed: {}", key_name),
                        )
                    })
                });
            task = task.chain(next);
        }
        task
    }

    fn release_key(
        &mut self,
        common: KeyEventCommon,
        key_widget_event: KeyWidgetEvent,
    ) -> Task<Message> {
        let modifier_state = to_modifier_state(&common.key_value);
        match modifier_state {
            s @ ModifierState::CapsLock => self.view_flags ^= s as u32,
            s => self.view_flags &= !(s as u32),
        };

        if let Some(key_state) = self.pressed_keys.remove(&common.key_name) {
            self.holding_key_state
                .take_if(|s| s.name == common.key_name);

            let pressed_time = key_state.pressed_time;
            let released_time = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);

            // shift may be used as a shortcut to switch the state of an input
            // method, We only send key pressed event to fcitx5, if the
            // pressing time is short enough. And we never send caps lock event.
            if modifier_state == ModifierState::CapsLock
                || (modifier_state == ModifierState::Shift && released_time - pressed_time > 500)
            {
                return Message::nothing();
            }
            let pressed_event_sent =
                modifier_state != ModifierState::NoState && modifier_state != ModifierState::Shift;
            let key_name = common.key_name.clone();
            if key_widget_event.cancelled {
                return if pressed_event_sent {
                    self.keyboard_backend
                        .process_key_events(
                            &mut self.keyboard_backend_state,
                            vec![ProcessKeyEventRequest {
                                key_value: key_state.selected_key_value,
                                is_release: true,
                                time: released_time,
                            }],
                            self.font,
                        )
                        .map(move |r| {
                            r.unwrap_or_else(|e| {
                                app::error_with_context(
                                    e,
                                    format!("send key released event failed: {}", key_name),
                                )
                            })
                        })
                } else {
                    Message::nothing()
                };
            }
            let mut reqs = Vec::with_capacity(2);
            if !pressed_event_sent {
                // press event has been sent when it is not NoState/Shift/CapsLock
                reqs.push(ProcessKeyEventRequest {
                    key_value: key_state.selected_key_value.clone(),
                    is_release: false,
                    time: pressed_time,
                });
            }
            reqs.push(ProcessKeyEventRequest {
                key_value: key_state.selected_key_value,
                is_release: true,
                time: released_time,
            });
            self.keyboard_backend
                .process_key_events(&mut self.keyboard_backend_state, reqs, self.font)
                .map(move |r| {
                    r.unwrap_or_else(|e| {
                        app::error_with_context(
                            e,
                            format!("send key released event failed: {}", key_name),
                        )
                    })
                })
        } else {
            Message::nothing()
        }
    }

    fn hold_key(
        &mut self,
        common: KeyEventCommon,
        key_widget_event: KeyWidgetEvent,
        pressed_time: u128,
    ) {
        if let Some(key_state) = self.pressed_keys.get(&common.key_name) {
            // check if the pressed time is the same
            if key_state.pressed_time != pressed_time {
                tracing::debug!(
                    "pressed_time is not equal: {}/{}, skip holding event.",
                    pressed_time,
                    key_state.pressed_time
                );
                return;
            }
        } else {
            return;
        }

        if let Some(holding_key_state) = &self.holding_key_state {
            tracing::warn!(
                "it can't be holding two keys at the same time, {} is already holding, holding {} will be skipped",
                holding_key_state.name,
                common.key_name
            );
            return;
        }

        if let Some(key) = self.keys.get(&*common.key_name) {
            if key.has_secondary() {
                self.holding_key_state = Some(HoldingKeyState {
                    name: common.key_name,
                    key_widget_event,
                    key: key.clone(),
                    flags: Vec::with_capacity(key.secondaries().len()),
                })
            }
        }
    }

    pub fn clear_fcitx5_hidden(&mut self) -> Task<Message> {
        if self.fcitx5_hidden != Fcitx5Hidden::Unset {
            // make sure to unset the flag.
            self.fcitx5_hidden = Fcitx5Hidden::Clearing;
            self.keyboard_backend
                .show_virtual_keyboard()
                .map(|r| match r {
                    Ok(_) => KeyboardEvent::UnsetFcitx5Hidden.into(),
                    Err(e) => app::error_with_context(e, "show virtual keyboard in pressing event"),
                })
        } else {
            Message::nothing()
        }
    }

    pub fn is_combo_mode(&self) -> bool {
        matches!(self.keyboard_backend_state.mode, Mode::Combo(_))
    }

    pub fn repeat_keys(&self) -> &[ComboKey] {
        &self.keyboard_backend_state.repeat_keys
    }

    pub fn repeat_serial(&self) -> u32 {
        self.keyboard_backend_state.repeat_serial
    }

    pub fn default_font(&self) -> Font {
        self.font
    }
}

#[derive(Clone, Debug)]
pub struct KeyEvent {
    common: KeyEventCommon,
    inner: KeyEventInner,
}

impl KeyEvent {
    fn new(common: KeyEventCommon, inner: KeyEventInner) -> Self {
        Self { common, inner }
    }
}

#[derive(Clone, Debug)]
struct KeyEventCommon {
    id: u8,
    key_name: Arc<str>,
    key_value: KeyValue,
}

impl KeyEventCommon {
    fn new(id: u8, key_name: Arc<str>, key_value: KeyValue) -> Self {
        Self {
            id,
            key_name,
            key_value,
        }
    }
}

#[derive(Clone, Debug)]
enum KeyEventInner {
    Pressed(KeyWidgetEvent),
    Holding(KeyWidgetEvent, u128),
    Released(KeyWidgetEvent),
    SelectSecondary,
    UnselectSecondary,
}

impl From<KeyEvent> for Message {
    fn from(value: KeyEvent) -> Self {
        Self::KeyEvent(value)
    }
}

#[derive(Clone, Debug)]
pub enum KeyboardEvent {
    UnsetFcitx5Hidden,
    ToggleComboMode,
    InsertComboKeyRelease,
    InsertComboKeyReleaseAll,
    RepeatComboKeys((u32, Duration)),
    StopRepeating,
}

impl From<KeyboardEvent> for Message {
    fn from(value: KeyboardEvent) -> Self {
        Self::KeyboardEvent(value)
    }
}

#[derive(Clone, Debug)]
enum Mode {
    Normal { modifiers: Arc<AtomicU32> },
    Combo(ComboState),
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal {
            modifiers: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug, Default)]
struct ComboState {
    keys: Vec<ComboKey>,
}

/// Because `KeyboardBackend` is shared elsewhere, I put its state here
#[derive(Default)]
struct KeyboardBackendState {
    /// record the key events to be repeated
    repeat_keys: Vec<ComboKey>,
    repeat_serial: u32,
    mode: Mode,
}

#[derive(Clone, Debug, Getters)]
pub struct KeyboardBackend {
    controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
    virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
    // Ensure the usage of virtual_keyboard_backend are serialized so that key events from
    // different messages do not interfere with each other
    virtual_keyboard_backend:
        Arc<IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>>,
}

impl KeyboardBackend {
    pub async fn new(
        modifier_workaround: bool,
        modifier_workaround_keycodes: Vec<u16>,
    ) -> Result<Self> {
        let connection = Connection::session().await?;
        let controller = Fcitx5ControllerServiceProxy::new(&connection).await?;
        let virtual_keyboard = Fcitx5VirtualKeyboardServiceProxy::new(&connection).await?;
        let virtual_keyboard_backend =
            Fcitx5VirtualKeyboardBackendServiceProxy::new(&connection).await?;
        // only enable fused backend, if `--modifier-workaround` is set and keycodes isn't empty
        if modifier_workaround && !modifier_workaround_keycodes.is_empty() {
            tracing::debug!(
                "Work in modifier workaround mode, keycodes: {:?}",
                modifier_workaround_keycodes
            );
            let virtual_keyboard = Arc::new(virtual_keyboard);
            let virtual_keyboard_backend = Arc::new(IcedFuturesMutex::new(
                WorkaroundFcitx5VirtualKeyboardBackendService::new(
                    virtual_keyboard.clone(),
                    virtual_keyboard_backend,
                    modifier_workaround_keycodes,
                ),
            ));
            Ok(Self {
                controller: Arc::new(controller),
                virtual_keyboard,
                virtual_keyboard_backend,
            })
        } else {
            tracing::debug!(
                "Work in normal mode: {}/{:?}",
                modifier_workaround,
                modifier_workaround_keycodes
            );
            let virtual_keyboard_backend =
                Arc::new(IcedFuturesMutex::new(virtual_keyboard_backend));
            Ok(Self {
                controller: Arc::new(controller),
                virtual_keyboard: Arc::new(virtual_keyboard),
                virtual_keyboard_backend,
            })
        }
    }

    pub fn new_with(
        controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
        virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
        virtual_keyboard_backend: Arc<
            IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>,
        >,
    ) -> Self {
        Self {
            controller,
            virtual_keyboard,
            virtual_keyboard_backend,
        }
    }

    fn candidate_area_state_message(
        candidate_text_list: Vec<Vec<(String, Option<Font>)>>,
    ) -> Message {
        Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(Arc::new(CandidateAreaState::new(
            candidate_text_list,
            false,
            false,
            0,
            0,
        )))
        .into()
    }

    fn combo_keys_message(combo_keys: &[ComboKey], default_font: Font) -> Message {
        let mut candidate_text_list = vec![];
        if !combo_keys.is_empty() {
            let mut text = vec![];
            for key in combo_keys {
                text.push((key.to_string(), Some(key.font().unwrap_or(default_font))));
                text.push((" + ".to_string(), Some(default_font)));
            }
            text.pop();
            candidate_text_list.push(text);
        }
        Self::candidate_area_state_message(candidate_text_list)
    }

    fn toggle_combo_mode(&self, state: &mut KeyboardBackendState) -> Task<Message> {
        match &mut state.mode {
            Mode::Normal { .. } => {
                state.mode = Mode::Combo(ComboState::default());
                Task::done(Self::candidate_area_state_message(vec![]))
            }
            Mode::Combo(combo_state) => {
                let keys = mem::take(&mut combo_state.keys);
                let task = self.process_combo_keys(keys.clone());
                if !keys.is_empty() {
                    state.repeat_keys = keys;
                }
                state.mode = Default::default();
                task.map(|r| match r {
                    Ok(_) => Self::candidate_area_state_message(vec![]),
                    Err(e) => app::error_with_context(e, "unable to send combo keys"),
                })
            }
        }
    }

    pub fn show_virtual_keyboard(&self) -> Task<ZbusResult<()>> {
        super::dbus_task(Cow::Borrowed(&self.virtual_keyboard), |s| async move {
            s.show_virtual_keyboard().await
        })
    }

    pub fn hide_virtual_keyboard(&self) -> Task<ZbusResult<()>> {
        #[derive(Clone)]
        struct Fcitx5VirtualKeyboardServiceExt {
            virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
            // Ensure the usage of virtual_keyboard_backend are serialized so that key events from
            // different messages do not interfere with each other
            virtual_keyboard_backend:
                Arc<IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>>,
        }
        super::dbus_task(
            Cow::Owned(Fcitx5VirtualKeyboardServiceExt {
                virtual_keyboard: self.virtual_keyboard.clone(),
                virtual_keyboard_backend: self.virtual_keyboard_backend.clone(),
            }),
            |ext: Fcitx5VirtualKeyboardServiceExt| async move {
                let mut virtual_keyboard_backend = ext.virtual_keyboard_backend.lock().await;
                virtual_keyboard_backend.reset_pressed_key_events().await?;
                ext.virtual_keyboard.hide_virtual_keyboard().await
            },
        )
    }

    pub fn sync_input_methods_and_current_im(&self) -> Task<Message> {
        super::call_dbus(
            &self.controller,
            "get input method group info and current im failed".to_string(),
            |s| async move {
                // if we fetch input methods and current input method in two message, in some cases, we will update current input method first. And it will fail because there is no input methods. So we put them in a call.
                let group_info = s.full_input_method_group_info("").await?;
                let input_method = s.current_input_method().await?;
                Ok(
                    ImEvent::UpdateImListAndCurrentIm(
                        group_info.into_input_methods(),
                        input_method,
                    )
                    .into(),
                )
            },
        )
    }

    pub fn current_input_method(&self) -> Task<ZbusResult<String>> {
        super::dbus_task(Cow::Borrowed(&self.controller), |s| async move {
            s.current_input_method().await
        })
    }

    pub fn set_current_im(&self, im: String) -> Task<ZbusResult<()>> {
        super::dbus_task(Cow::Borrowed(&self.controller), |s| async move {
            s.set_current_im(&im).await
        })
    }

    pub fn prev_page(&self, page_index: i32) -> Task<ZbusResult<()>> {
        super::dbus_task(
            Cow::Borrowed(&self.virtual_keyboard_backend),
            |s| async move {
                let s = s.lock().await;
                s.prev_page(page_index).await
            },
        )
    }

    pub fn next_page(&self, page_index: i32) -> Task<ZbusResult<()>> {
        super::dbus_task(
            Cow::Borrowed(&self.virtual_keyboard_backend),
            |s| async move {
                let s = s.lock().await;
                s.next_page(page_index).await
            },
        )
    }

    fn select_candidate(&self, state: &mut KeyboardBackendState, cursor: usize) -> Task<Message> {
        match &mut state.mode {
            Mode::Normal { .. } => super::call_dbus(
                &self.virtual_keyboard_backend,
                format!("select candidate {} failed", cursor),
                |s| async move {
                    let s = s.lock().await;
                    s.select_candidate(cursor as i32).await?;
                    Ok(Message::Nothing)
                },
            ),
            Mode::Combo(combo_state) => {
                let task = self
                    .process_combo_keys(combo_state.keys.clone())
                    .map(|r| match r {
                        Ok(_) => Self::candidate_area_state_message(vec![]),
                        Err(e) => app::error_with_context(e, "error to process combo keys"),
                    });
                mem::swap(&mut state.repeat_keys, &mut combo_state.keys);
                combo_state.keys.clear();
                task
            }
        }
    }

    fn process_key_events_direct(
        &self,
        reqs: Vec<ProcessKeyEventRequest>,
        modifiers: Option<Arc<AtomicU32>>,
    ) -> Task<ZbusResult<()>> {
        super::dbus_task(
            Cow::Borrowed(&self.virtual_keyboard_backend),
            |s| async move {
                let mut virtual_keyboard_backend = s.lock().await;
                let mut m = if let Some(modifiers) = &modifiers {
                    modifiers.load(Ordering::SeqCst)
                } else {
                    0
                };
                for req in reqs {
                    let modifier_state = to_modifier_state(&req.key_value);
                    if req.is_release {
                        match modifier_state {
                            s @ ModifierState::CapsLock => m ^= s as u32,
                            s => m &= !(s as u32),
                        };
                    } else {
                        if modifier_state != ModifierState::CapsLock {
                            m |= modifier_state as u32;
                        }
                    }
                    // if the key event is not handled by input method, fcitx5
                    // will forward the key event. For example, when you press
                    // a `k` and you are using `keyboard-us` as the input
                    // method, fcitx5 will forward the key event to a wayland
                    // server. In the implementation of kwin, it will only
                    // handle limited keysyms if the wayland client doesn't
                    // support text-input-v1 or text-input-v2
                    // (src/inputmethod.cpp:keysymReceived).
                    //
                    // So I add keycodes to each key, if the key event
                    // contains a key code, I will send the key code instead
                    // of key value to fcitx5.
                    let (keycode, modifiers) = if let Some(keycode) = req.key_value.keycode() {
                        (keycode, 0)
                    } else {
                        // TODO how will modifiers be used?
                        (0, m)
                    };

                    let should_shift_release =
                        if !req.is_release && modifier_state != ModifierState::Shift {
                            if keycode < 0 && !ModifierState::Shift.is_set(m) {
                                // send a shift press event
                                Some(false)
                            } else if keycode > 0 && ModifierState::Shift.is_set(m) {
                                // send a shift release event
                                Some(true)
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    // Send shift before press
                    if let Some(should_shift_release) = should_shift_release {
                        virtual_keyboard_backend
                            .process_key_event(
                                Keysym::Shift_L.raw(),
                                KEYCODE_LEFT_SHIFT as u32,
                                0,
                                should_shift_release,
                                // timestamp with millisecond granularity
                                req.time as u32 - 1,
                            )
                            .await?
                    }
                    virtual_keyboard_backend
                        .process_key_event(
                            req.key_value.keysym().raw(),
                            keycode.unsigned_abs() as u32,
                            modifiers,
                            req.is_release,
                            // timestamp with millisecond granularity
                            req.time as u32,
                        )
                        .await?;
                    // Send shift after press
                    if let Some(should_shift_release) = should_shift_release {
                        virtual_keyboard_backend
                            .process_key_event(
                                Keysym::Shift_L.raw(),
                                KEYCODE_LEFT_SHIFT as u32,
                                0,
                                !should_shift_release,
                                // timestamp with millisecond granularity
                                req.time as u32 + 1,
                            )
                            .await?
                    }
                }
                if let Some(modifiers) = &modifiers {
                    modifiers.store(m, Ordering::SeqCst);
                }
                Ok(())
            },
        )
    }

    fn process_key_events(
        &self,
        state: &mut KeyboardBackendState,
        reqs: Vec<ProcessKeyEventRequest>,
        default_font: Font,
    ) -> Task<ZbusResult<Message>> {
        match &mut state.mode {
            Mode::Normal { modifiers } => {
                let mut last_release = None;
                for req in &reqs {
                    if req.is_release {
                        last_release = Some(&req.key_value);
                    }
                }
                if let Some(last_release) = last_release {
                    state.repeat_keys.clear();
                    state.repeat_keys.push(ComboKey::Key(last_release.clone()));
                }
                self.process_key_events_direct(reqs, Some(modifiers.clone()))
                    .map(|r| r.map(|_| Message::Nothing))
            }
            Mode::Combo(combo_state) => {
                for req in reqs {
                    if !req.is_release {
                        if req.key_value.keysym() == Keysym::BackSpace
                            || req.key_value.keycode() == Some(22)
                        {
                            // Delete the last key
                            combo_state.keys.pop();
                        } else {
                            combo_state.keys.push(ComboKey::Key(req.key_value));
                        }
                    }
                }
                Task::done(Ok(Self::combo_keys_message(
                    &combo_state.keys,
                    default_font,
                )))
            }
        }
    }

    fn process_combo_keys(&self, combo_keys: Vec<ComboKey>) -> Task<ZbusResult<()>> {
        // 20ms
        const INTERVAL: u128 = 20_000_000;

        let mut reqs = vec![];
        let mut stack: Vec<Option<KeyValue>> = vec![];
        let mut time = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);

        // Generate key events according to combo keys
        for combo_key in combo_keys {
            match combo_key {
                ComboKey::Key(key_value) => {
                    // Check if the key is pressed already, release it before pressing it
                    for pressed_key in &mut stack {
                        if let Some(p) = pressed_key {
                            if p.keycode() == key_value.keycode()
                                || p.keysym() == key_value.keysym()
                            {
                                reqs.push(ProcessKeyEventRequest {
                                    key_value: p.clone(),
                                    is_release: true,
                                    time,
                                });
                                time += INTERVAL;
                                pressed_key.take();
                            }
                        }
                    }
                    stack.push(Some(key_value.clone()));
                    reqs.push(ProcessKeyEventRequest {
                        key_value,
                        is_release: false,
                        time,
                    });
                    time += INTERVAL;
                }
                ComboKey::Release => {
                    if let Some(Some(key_value)) = stack.pop() {
                        reqs.push(ProcessKeyEventRequest {
                            key_value,
                            is_release: true,
                            time,
                        });
                        time += INTERVAL;
                    }
                }
                ComboKey::ReleaseAll => {
                    while let Some(key_value) = stack.pop() {
                        if let Some(key_value) = key_value {
                            reqs.push(ProcessKeyEventRequest {
                                key_value,
                                is_release: true,
                                time,
                            });
                            time += INTERVAL;
                        }
                    }
                }
            }
        }
        while let Some(key_value) = stack.pop() {
            if let Some(key_value) = key_value {
                reqs.push(ProcessKeyEventRequest {
                    key_value,
                    is_release: true,
                    time,
                });
                time += INTERVAL;
            }
        }

        self.process_key_events_direct(reqs, None)
    }

    fn insert_combo_key(
        &self,
        state: &mut KeyboardBackendState,
        combo_key: ComboKey,
        default_font: Font,
    ) -> Task<Message> {
        match &mut state.mode {
            Mode::Normal { .. } => Message::nothing(),
            Mode::Combo(combo_state) => {
                combo_state.keys.push(combo_key);
                Task::done(Self::combo_keys_message(&combo_state.keys, default_font))
            }
        }
    }
}

struct ProcessKeyEventRequest {
    key_value: KeyValue,
    is_release: bool,
    time: u128,
}

fn to_modifier_state(key_value: &KeyValue) -> ModifierState {
    match key_value.keysym() {
        Keysym::Shift_L | Keysym::Shift_R => ModifierState::Shift,
        Keysym::Caps_Lock => ModifierState::CapsLock,
        Keysym::Control_L | Keysym::Control_R => ModifierState::Ctrl,
        Keysym::Alt_L | Keysym::Alt_R => ModifierState::Alt,
        Keysym::Num_Lock => ModifierState::NumLock,
        Keysym::Super_L | Keysym::Super_R => ModifierState::Super,
        _ => ModifierState::NoState,
    }
}
