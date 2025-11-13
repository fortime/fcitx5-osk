use std::{
    collections::HashMap,
    ops::DerefMut as _,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use iced::{
    alignment::{Horizontal, Vertical},
    futures::lock::Mutex as IcedFuturesMutex,
    widget::{container::Style as ContainerStyle, text::Shaping, Column, Container, Row, Text},
    Element, Font, Padding, Task,
};
use xkeysym::Keysym;

use crate::{
    app::Message,
    dbus::client::{
        Fcitx5Services, Fcitx5VirtualKeyboardServiceExt, IFcitx5VirtualKeyboardBackendService,
        IFcitx5VirtualKeyboardService,
    },
    font,
    key_set::{Key, KeyValue, ThinKeyValue},
    layout::KeyAreaLayout,
    store::Store,
    widget::{Key as KeyWidget, KeyEvent as KeyWidgetEvent, PopupKey},
};

const TEXT_PADDING_LENGTH: u16 = 3;

/// #define KEY_LEFTSHIFT       42, val + 8
const KEYCODE_LEFT_SHIFT: u32 = 50;

const BORDER_RADIUS: f32 = 5.;

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
    selected_key_value: ThinKeyValue,
}

struct HoldingKeyState {
    name: Arc<str>,
    key_widget_event: KeyWidgetEvent,
    key: Key,
    // The order of SelectSecondary/UnselectSecondary is uncertain when moving between popup keys,
    // we use this flags to record which key is selected, once it is empty, we will use the primary
    // keysym.
    flags: Vec<ThinKeyValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fcitx5Hidden {
    Unset,
    Clearing,
    Set,
}

pub struct KeyboardState {
    id: u8,
    modifiers: u32,
    primary_text_size_u: u16,
    secondary_text_size_u: u16,
    font: Font,
    keys: HashMap<String, Key>,
    pressed_keys: HashMap<Arc<str>, KeyState>,
    holding_timeout: Duration,
    holding_key_state: Option<HoldingKeyState>,
    popup_key_width_u: u16,
    popup_key_height_u: u16,
    /// if there is no indicator and fcitx5 hides virtual keyboard, we won't hide the keyboard,
    /// instead we set this flag, and tell fcitx5 to show virtual keyboard when there is any key
    /// pressing event.
    fcitx5_hidden: Fcitx5Hidden,
    fcitx5_services: Fcitx5Services,
}

impl KeyboardState {
    pub fn new(
        holding_timeout: Duration,
        key_area_layout: &KeyAreaLayout,
        store: &Store,
        fcitx5_services: Fcitx5Services,
    ) -> Self {
        let mut res = Self {
            id: 0,
            // always virtual
            modifiers: Default::default(),
            primary_text_size_u: Default::default(),
            secondary_text_size_u: Default::default(),
            font: Default::default(),
            keys: HashMap::new(),
            pressed_keys: HashMap::new(),
            holding_timeout,
            holding_key_state: None,
            popup_key_width_u: 0,
            popup_key_height_u: 0,
            fcitx5_hidden: Fcitx5Hidden::Unset,
            fcitx5_services,
        };
        res.update_key_area_layout(key_area_layout, store);
        res
    }

    pub fn update_key_area_layout(&mut self, key_area_layout: &KeyAreaLayout, store: &Store) {
        self.id = self.id.wrapping_add(1);
        self.modifiers = 0;
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
    }

    pub fn on_event(&mut self, event: KeyboardEvent) -> Task<Message> {
        match event {
            KeyboardEvent::UnsetFcitx5Hidden => {
                if self.fcitx5_hidden == Fcitx5Hidden::Clearing {
                    self.fcitx5_hidden = Fcitx5Hidden::Unset;
                }
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
        unit: u16,
        border_radius: f32,
    ) -> PopupKey<'a, Message> {
        let common =
            KeyEventCommon::new(self.id, holding_key_state.name.clone(), key_value.to_thin());
        PopupKey::new(
            Text::new(key_value.symbol())
                .shaping(Shaping::Advanced)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .font(key_value.font().unwrap_or(self.font))
                .size(self.primary_text_size_u * unit),
            holding_key_state.key_widget_event.finger,
            border_radius,
        )
        .width(self.popup_key_width_u * unit)
        .height(self.popup_key_height_u * unit)
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
                key_state.selected_key_value = key_value;
                holding_key_state.flags.push(key_value);
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
                        key_state.selected_key_value = *key_value;
                    } else {
                        // set key_value to the primary one.
                        let is_shift_set = ModifierState::Shift.is_set(self.modifiers);
                        let is_caps_lock_set = ModifierState::CapsLock.is_set(self.modifiers);
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

    pub fn key(&self, key_name: Arc<str>, unit: u16, size: (u16, u16)) -> Element<Message> {
        let (width, height) = size;
        let (inner_width, inner_height) = (
            width - TEXT_PADDING_LENGTH * 2,
            height - TEXT_PADDING_LENGTH * 2,
        );

        let (content, press_cb, release_cb) = if let Some(key) = self.keys.get(&*key_name) {
            let is_shift_set = ModifierState::Shift.is_set(self.modifiers);
            let is_caps_lock_set = ModifierState::CapsLock.is_set(self.modifiers);
            let secondary_height = inner_height / 3;
            let primary_height = inner_height - secondary_height;
            // It's related to the conversion between float and int, if we don't minus 1, it may
            // be too large in float, and the text can't be shown
            let secondary_text_size = (secondary_height - 1).min(self.secondary_text_size_u * unit);
            let primary_text_size = (primary_height - 1).min(self.primary_text_size_u * unit);
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
                    .size(secondary_text_size as f32)
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
                        .size(primary_text_size as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            } else {
                // If there is no secondary, set it in the middle of the key
                column = column.push(
                    middle
                        .width(inner_width)
                        .height(inner_height)
                        .size(primary_text_size as f32)
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
        KeyWidget::new(content, BORDER_RADIUS)
            .on_press_with(press_cb)
            .on_release_with(release_cb)
            .padding(Padding::new(TEXT_PADDING_LENGTH as f32))
            .width(width)
            .height(height)
            .into()
    }

    pub fn popup_overlay(&self, unit: u16, size: (u16, u16)) -> Option<Element<Message>> {
        const MARGIN_U: u16 = 1;
        let (width, height) = size;

        let holding_key_state = self.holding_key_state.as_ref()?;

        let is_shift_set = ModifierState::Shift.is_set(self.modifiers);
        let is_caps_lock_set = ModifierState::CapsLock.is_set(self.modifiers);

        let key = &holding_key_state.key;
        let mut row = Row::new();
        let mut skip = 0;
        let mut popup_key_area_width = 0;
        if Key::is_shifted(is_shift_set, is_caps_lock_set) {
            row =
                row.push(self.new_popup_key(holding_key_state, key.primary(), unit, BORDER_RADIUS));
            skip = 1;
            popup_key_area_width += self.popup_key_width_u * unit;
        }
        for secondary in key.secondaries().iter().skip(skip) {
            row = row.push(self.new_popup_key(holding_key_state, secondary, unit, BORDER_RADIUS));
            popup_key_area_width += self.popup_key_width_u * unit;
        }

        // calculate position.
        let bounds = &holding_key_state.key_widget_event.bounds;
        let mut left_x = bounds.x as u16;
        if left_x + popup_key_area_width > width {
            left_x = width.saturating_sub(popup_key_area_width);
        }
        let mut top_y = bounds.y as u16;
        if top_y > self.popup_key_height_u * unit + MARGIN_U * unit {
            top_y -= self.popup_key_height_u * unit + MARGIN_U * unit;
        } else {
            top_y += bounds.height as u16 + MARGIN_U * unit;
        }

        // calculate padding.
        let padding = Padding::default().left(left_x as f32).top(top_y as f32);
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
    pub(super) fn update_fcitx5_services(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = fcitx5_services;
    }

    fn fcitx5_virtual_keyboard_backend_service(
        &self,
    ) -> &Arc<IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>> {
        self.fcitx5_services.virtual_keyboard_backend()
    }

    fn fcitx5_virtual_keyboard_service(&self) -> &Fcitx5VirtualKeyboardServiceExt {
        self.fcitx5_services.virtual_keyboard()
    }

    fn press_key(
        &mut self,
        common: KeyEventCommon,
        key_widget_event: KeyWidgetEvent,
    ) -> Task<Message> {
        let modifier_state = to_modifier_state(common.key_value);
        if modifier_state != ModifierState::CapsLock {
            self.modifiers |= modifier_state as u32;
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
                    selected_key_value: common.key_value,
                }
            });
        }
        let mut task = self.clear_fcitx5_hidden();
        if modifier_state == ModifierState::NoState && !contains {
            let holding_timeout = self.holding_timeout;
            let next = Task::future(async move {
                tokio::time::sleep(holding_timeout).await;
                KeyEvent::new(
                    common,
                    KeyEventInner::Holding(key_widget_event, pressed_time),
                )
                .into()
            });
            task = task.chain(next);
        } else if modifier_state != ModifierState::CapsLock
            && modifier_state != ModifierState::Shift
        {
            // Don't send caps lock and shift state.
            let modifiers =
                self.modifiers & !(ModifierState::CapsLock as u32) & !(ModifierState::Shift as u32);
            let next = super::call_dbus(
                self.fcitx5_virtual_keyboard_backend_service(),
                format!("send key pressed event failed: {}", common.key_name),
                |s| async move {
                    let mut s = s.lock().await;

                    let (keyval, keycode, modifiers) =
                        if let Some(keycode) = common.key_value.keycode() {
                            let keyval = u32::from(common.key_value.keysym());
                            (keyval, keycode, 0)
                        } else {
                            let keyval = u32::from(common.key_value.keysym());
                            // TODO how will modifiers be used?
                            (keyval, 0, modifiers)
                        };
                    s.process_key_event(
                        keyval,
                        keycode.unsigned_abs() as u32,
                        modifiers,
                        false,
                        // timestamp with millisecond granularity
                        pressed_time as u32,
                    )
                    .await?;
                    Ok(Message::Nothing)
                },
            );
            task = task.chain(next);
        }
        task
    }

    fn release_key(
        &mut self,
        common: KeyEventCommon,
        key_widget_event: KeyWidgetEvent,
    ) -> Task<Message> {
        let modifier_state = to_modifier_state(common.key_value);
        match modifier_state {
            s @ ModifierState::CapsLock => self.modifiers ^= s as u32,
            s => self.modifiers &= !(s as u32),
        };

        if let Some(key_state) = self.pressed_keys.remove(&common.key_name) {
            self.holding_key_state
                .take_if(|s| s.name == common.key_name);

            let pressed_time = key_state.pressed_time;
            let released_time = UNIX_EPOCH.elapsed().map(|d| d.as_millis()).unwrap_or(0);

            if modifier_state == ModifierState::CapsLock
                || (modifier_state == ModifierState::Shift && released_time - pressed_time > 500)
            {
                // shift may be used as a shortcut to switch the state of an input method,
                // We only send key pressed event to fcitx5, if the pressing time is short enough.
                // And we never send caps lock event.
                return Message::nothing();
            }

            // not send caps lock and shift state.
            let modifiers =
                self.modifiers & !(ModifierState::CapsLock as u32) & !(ModifierState::Shift as u32);

            super::call_dbus(
                self.fcitx5_virtual_keyboard_backend_service(),
                format!(
                    "send key pressed/released event failed: {}",
                    common.key_name
                ),
                |s| async move {
                    let mut s = s.lock().await;

                    on_key_release(
                        s.deref_mut(),
                        &key_state,
                        modifier_state,
                        modifiers,
                        pressed_time,
                        released_time,
                        key_widget_event.cancelled,
                    )
                    .await
                },
            )
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
            super::call_dbus(
                self.fcitx5_virtual_keyboard_service(),
                "show virtual keyboard in pressing event",
                |s| async move {
                    s.show_virtual_keyboard().await?;
                    Ok(KeyboardEvent::UnsetFcitx5Hidden.into())
                },
            )
        } else {
            Message::nothing()
        }
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
    key_value: ThinKeyValue,
}

impl KeyEventCommon {
    fn new(id: u8, key_name: Arc<str>, key_value: ThinKeyValue) -> Self {
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
}

impl From<KeyboardEvent> for Message {
    fn from(value: KeyboardEvent) -> Self {
        Self::KeyboardEvent(value)
    }
}

fn to_modifier_state(key_value: ThinKeyValue) -> ModifierState {
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

async fn on_key_release(
    s: &mut (dyn IFcitx5VirtualKeyboardBackendService + Send + Sync),
    key_state: &KeyState,
    modifier_state: ModifierState,
    modifiers: u32,
    pressed_time: u128,
    released_time: u128,
    cancelled: bool,
) -> Result<Message> {
    // if the key event is not handled by input method, fcitx5 will forward the key
    // event. For example, when you press a `k` and you are using `keyboard-us` as
    // the input method, fcitx5 will forward the key event to a wayland server. In
    // the implementation of kwin, it will only handle limited keysyms if the
    // wayland client doesn't support text-input-v1 or text-input-v2
    // (src/inputmethod.cpp:keysymReceived).
    //
    // So I add keycodes to each key, if the key event contains a key code, I will
    // send the key code instead of key value to fcitx5.
    let (keyval, keycode, modifiers) = if let Some(keycode) = key_state.selected_key_value.keycode()
    {
        // it looks like some input methods that needs keysym to work.
        let keyval = u32::from(key_state.selected_key_value.keysym());
        (keyval, keycode, 0)
    } else {
        let keyval = u32::from(key_state.selected_key_value.keysym());
        (keyval, 0, modifiers)
    };
    let send_shift = keycode < 0;
    let keycode = keycode.unsigned_abs() as u32;
    // timestamp with millisecond granularity
    let pressed_time = pressed_time as u32;
    let released_time = released_time as u32;
    let pressed_event_sent =
        modifier_state != ModifierState::NoState && modifier_state != ModifierState::Shift;
    if cancelled {
        if pressed_event_sent {
            s.process_key_event(keyval, keycode, modifiers, true, released_time)
                .await?;
        }
        return Ok(Message::Nothing);
    }
    if send_shift {
        // send a shift press event
        s.process_key_event(0, KEYCODE_LEFT_SHIFT, 0, false, pressed_time - 1)
            .await?;
    }
    if !pressed_event_sent {
        // press event has been sent when it is not NoState/Shift/CapsLock
        s.process_key_event(keyval, keycode, modifiers, false, pressed_time)
            .await?;
    }
    s.process_key_event(keyval, keycode, modifiers, true, released_time)
        .await?;
    if send_shift {
        // send a shift release event
        s.process_key_event(0, KEYCODE_LEFT_SHIFT, 0, true, released_time)
            .await?;
    }
    Ok(Message::Nothing)
}
