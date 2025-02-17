use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use iced::{
    alignment::{Horizontal, Vertical},
    futures::channel::mpsc::UnboundedSender,
    touch::Finger,
    widget::{container::Style as ContainerStyle, Column, Container, Row, Text},
    Element, Font, Padding, Task,
};
use xkeysym::Keysym;
use zbus::Connection;

use crate::{
    app::Message,
    dbus::{
        client::{Fcitx5Services, Fcitx5VirtualKeyboardBackendServiceProxy},
        server::Fcitx5VirtualkeyboardImPanelService,
    },
    font,
    key_set::Key,
    layout::{KeyAreaLayout, KeyManager},
    store::Store,
    widget::{Key as KeyWidget, KeyEvent as KeyWidgetEvent, PopupKey},
};

const TEXT_PADDING_LENGTH: u16 = 5;

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
    pressed_time: u64,
    primary_keysym: Keysym,
    selected_keysym: Keysym,
}

struct HoldingKeyState {
    name: Arc<String>,
    key_widget_event: KeyWidgetEvent,
    key: Key,
}

pub struct KeyboardState {
    id: u8,
    modifiers: u32,
    primary_text_size: u16,
    secondary_text_size: u16,
    font: Font,
    keys: HashMap<String, Key>,
    pressed_keys: HashMap<Arc<String>, KeyState>,
    holding_timeout: Duration,
    holding_key_state: Option<HoldingKeyState>,
    /// To avoid capturing the lifetime of this object, we seperate the connection process into
    /// multiple steps. In theory, there would be multiple creations of connection in the same
    /// time, we only keep the connection with the correct id only.
    dbus_service_token: u8,
    dbus_service_connection: Option<Connection>,
    fcitx5_services: Option<Fcitx5Services>,
}

impl KeyboardState {
    pub fn new(holding_timeout: Duration, key_area_layout: &KeyAreaLayout, store: &Store) -> Self {
        let mut res = Self {
            id: 0,
            // always virtual
            modifiers: Default::default(),
            primary_text_size: Default::default(),
            secondary_text_size: Default::default(),
            font: Default::default(),
            keys: HashMap::new(),
            pressed_keys: HashMap::new(),
            holding_timeout,
            holding_key_state: None,
            dbus_service_token: 0,
            dbus_service_connection: None,
            fcitx5_services: None,
        };
        res.update_key_area_layout(key_area_layout, store);
        res
    }

    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    pub fn update_key_area_layout(&mut self, key_area_layout: &KeyAreaLayout, store: &Store) {
        self.id = self.id.wrapping_add(1);
        self.modifiers = 0;
        self.primary_text_size = key_area_layout.primary_text_size();
        self.secondary_text_size = key_area_layout.secondary_text_size();
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
            .map(|n| font::load(&n))
            .unwrap_or_default();
    }

    pub fn start_dbus_service(&mut self, tx: UnboundedSender<Message>) -> Task<Message> {
        // drop the old one
        let _ = self.dbus_service_connection.take();

        self.dbus_service_token = self.dbus_service_token.wrapping_add(1);

        let new_dbus_service_token = self.dbus_service_token;
        Task::perform(
            async move {
                tracing::debug!("start dbus service: {}", new_dbus_service_token);
                let conn = Connection::session().await?;
                let s = Fcitx5VirtualkeyboardImPanelService::new(tx);
                s.start(&conn)
                    .await
                    .context("failed to start dbus service")?;
                Ok((new_dbus_service_token, conn))
            },
            |res: Result<_>| match res {
                Ok((id, connection)) => StartDbusServiceEvent::Started(id, connection).into(),
                Err(e) => super::fatal(e),
            },
        )
    }

    pub fn set_dbus_service_connection(
        &mut self,
        dbus_service_token: u8,
        connection: Connection,
    ) -> bool {
        if dbus_service_token != self.dbus_service_token {
            tracing::warn!(
                "concurrency creation of dbus service, connection is dropped: {}/{:?}",
                dbus_service_token,
                connection
            );
            false
        } else {
            self.dbus_service_connection = Some(connection);
            true
        }
    }
}

// call fcitx5
impl KeyboardState {
    fn fcitx5_virtual_keyboard_backend_service(
        &self,
    ) -> Option<&Fcitx5VirtualKeyboardBackendServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::virtual_keyboard_backend)
    }

    pub fn on_event(&mut self, event: KeyEvent) -> Task<Message> {
        match event {
            KeyEvent::Pressed(id, key_name, key_widget_event, keysym) => {
                self.press_key(id, key_name, key_widget_event, keysym)
            }
            KeyEvent::Holding(id, key_name, key_widget_event) => {
                self.hold_key(id, key_name, key_widget_event);
                Task::none()
            }
            KeyEvent::Released(id, key_name, keysym) => self.release_key(id, key_name, keysym),
            KeyEvent::SelectSecondary() => {
                todo!()
            }
        }
    }

    fn press_key(
        &mut self,
        id: u8,
        key_name: Arc<String>,
        key_widget_event: KeyWidgetEvent,
        keysym: Keysym,
    ) -> Task<Message> {
        let mut task = Task::none();

        if id != self.id {
            return task;
        }

        let modifier_state = to_modifier_state(keysym);
        if modifier_state != ModifierState::CapsLock {
            self.modifiers |= modifier_state as u32;
        }

        let key_state = self.pressed_keys.entry(key_name.clone());
        let mut contains = true;
        {
            let contains = &mut contains;
            key_state.or_insert_with(|| {
                *contains = false;
                let pressed_time = UNIX_EPOCH.elapsed().map(|d| d.as_secs()).unwrap_or(0);
                KeyState {
                    pressed_time,
                    primary_keysym: keysym,
                    selected_keysym: keysym,
                }
            });
        }
        if modifier_state == ModifierState::NoState {
            if !contains {
                let holding_timeout = self.holding_timeout;
                task = task.chain(Task::future(async move {
                    tokio::time::sleep(holding_timeout).await;
                    KeyEvent::Holding(id, key_name, key_widget_event).into()
                }));
            }
        }

        task
    }

    fn release_key(&mut self, id: u8, key_name: Arc<String>, keysym: Keysym) -> Task<Message> {
        let mut task = Task::none();

        if id != self.id {
            return task;
        }

        let modifier_state = to_modifier_state(keysym);
        match modifier_state {
            s @ ModifierState::CapsLock => self.modifiers ^= s as u32,
            s @ _ => self.modifiers &= !(s as u32),
        };

        match (modifier_state, self.pressed_keys.remove(&key_name)) {
            (ModifierState::NoState, Some(key_state)) => {
                self.holding_key_state.take_if(|s| s.name == key_name);

                let keyval = u32::from(key_state.selected_keysym);
                let pressed_time = key_state.pressed_time as u32;
                let released_time = UNIX_EPOCH.elapsed().map(|d| d.as_secs()).unwrap_or(0) as u32;
                let modifiers = self.modifiers;
                task = task
                    .chain(super::call_fcitx5(
                        self.fcitx5_virtual_keyboard_backend_service(),
                        format!("send key pressed event failed: {key_name}"),
                        |s| async move {
                            s.process_key_event(keyval, 0, modifiers, false, pressed_time)
                                .await?;
                            Ok(Message::Nothing)
                        },
                    ))
                    .chain(super::call_fcitx5(
                        self.fcitx5_virtual_keyboard_backend_service(),
                        format!("send key released event failed: {key_name}"),
                        |s| async move {
                            s.process_key_event(keyval, 0, modifiers, true, released_time)
                                .await?;
                            Ok(Message::Nothing)
                        },
                    ));
            }
            _ => {}
        }

        task
    }

    fn hold_key(&mut self, id: u8, key_name: Arc<String>, key_widget_event: KeyWidgetEvent) {
        if id != self.id {
            return;
        }

        // TODO check if the pressed time is the same
        if !self.pressed_keys.contains_key(&key_name) {
            return;
        }

        if let Some(holding_key_state) = &self.holding_key_state {
            tracing::warn!("it can't be holding two keys at the same time, {} is already holding, holding {} will be skipped", holding_key_state.name, key_name);
            return;
        }

        if let Some(key) = self.keys.get(&*key_name) {
            if key.has_secondary() {
                self.holding_key_state = Some(HoldingKeyState {
                    name: key_name,
                    key_widget_event,
                    key: key.clone(),
                })
            }
        }
    }
}

impl KeyManager for KeyboardState {
    type Message = Message;

    fn key(&self, key_name: Arc<String>, unit: u16, size_p: (u16, u16)) -> Element<Self::Message> {
        let (width_p, height_p) = size_p;
        let (inner_width_p, inner_height_p) = (
            width_p - TEXT_PADDING_LENGTH * 2,
            height_p - TEXT_PADDING_LENGTH * 2,
        );

        let (content, press_cb, release_cb) = if let Some(key) = self.keys.get(&*key_name) {
            let is_shift_set = ModifierState::Shift.is_set(self.modifiers);
            let is_caps_lock_set = ModifierState::CapsLock.is_set(self.modifiers);
            let secondary_height_p = inner_height_p / 4;
            let primary_height_p = inner_height_p - 2 * secondary_height_p;
            let mut column: Column<Self::Message> = Column::new();
            let top = Text::new(key.secondary_text(is_shift_set, is_caps_lock_set));
            let middle = Text::new(key.primary_text(is_shift_set, is_caps_lock_set));
            let keysym = key.keysym(is_shift_set, is_caps_lock_set);
            column = column
                .push(
                    top.width(inner_width_p)
                        .height(secondary_height_p)
                        .font(self.font)
                        .size((self.secondary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Right),
                )
                .push(
                    middle
                        .width(inner_width_p)
                        .height(primary_height_p)
                        .font(self.font)
                        .size((self.primary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            let id = self.id;
            let press_key_name = key_name.clone();
            (
                Element::from(column),
                Some(move |key_widget_event| {
                    Message::from(KeyEvent::Pressed(
                        id,
                        press_key_name.clone(),
                        key_widget_event,
                        keysym,
                    ))
                }),
                Some(move |_key_widget_event| {
                    Message::from(KeyEvent::Released(id, key_name.clone(), keysym))
                }),
            )
        } else {
            tracing::debug!("{key_name} is not found");
            (Element::from(Text::new("")), None, None)
        };
        KeyWidget::new(content)
            .on_press_with(press_cb)
            .on_release_with(release_cb)
            .padding(Padding::new(TEXT_PADDING_LENGTH as f32))
            .width(width_p)
            .height(height_p)
            .into()
    }

    fn popup_overlay(&self, unit: u16, size_p: (u16, u16)) -> Option<Element<Self::Message>> {
        fn new_popup_key<'a>(
            holding_key_state: &'a HoldingKeyState,
            symbol: &'a String,
            font: Font,
            text_size: u16,
            width_p: u16,
            height_p: u16,
        ) -> PopupKey<'a, Message> {
            PopupKey::new(
                Text::new(symbol)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center)
                    .font(font)
                    .size(text_size),
                holding_key_state.key_widget_event.finger.clone(),
            )
            .width(width_p)
            .height(height_p)
            // TODO
            .on_enter(Message::Nothing)
            .on_exit(Message::Nothing)
        }

        let (width_p, height_p) = size_p;

        let holding_key_state = self.holding_key_state.as_ref()?;

        let is_shift_set = ModifierState::Shift.is_set(self.modifiers);
        let is_caps_lock_set = ModifierState::CapsLock.is_set(self.modifiers);

        let key = &holding_key_state.key;
        let mut row = Row::new();
        let mut skip = 0;
        let text_size = self.primary_text_size * unit;
        // TODO config
        let popupkey_width_p = 4 * unit;
        let popupkey_height_p = 4 * unit;
        let mut popupkey_area_width_p = 0;
        if Key::is_shifted(is_shift_set, is_caps_lock_set) {
            row = row.push(new_popup_key(
                holding_key_state,
                key.primary().symbol(),
                self.font,
                text_size,
                popupkey_width_p,
                popupkey_height_p,
            ));
            skip = 1;
            popupkey_area_width_p += popupkey_width_p;
        }
        for secondary in key.secondaries().iter().skip(skip) {
            row = row.push(new_popup_key(
                holding_key_state,
                secondary.symbol(),
                self.font,
                text_size,
                popupkey_width_p,
                popupkey_height_p,
            ));
            popupkey_area_width_p += popupkey_width_p;
        }

        // calculate position.
        let bounds = &holding_key_state.key_widget_event.bounds;
        let mut left_x = bounds.x as u16;
        if left_x + popupkey_area_width_p > width_p {
            left_x = width_p.checked_sub(popupkey_area_width_p).unwrap_or(0);
        }
        let mut top_y = bounds.y as u16;
        if top_y > popupkey_height_p {
            top_y -= popupkey_height_p;
        } else {
            top_y += bounds.height as u16;
        }

        // calculate padding.
        let padding = Padding::default().left(left_x as f32).top(top_y as f32);
        Some(
            Container::new(row)
                .padding(padding)
                .width(width_p)
                .height(height_p)
                .into(),
        )
    }
}

#[derive(Clone, Debug)]
pub enum KeyEvent {
    Pressed(u8, Arc<String>, KeyWidgetEvent, Keysym),
    Holding(u8, Arc<String>, KeyWidgetEvent),
    Released(u8, Arc<String>, Keysym),
    SelectSecondary(),
}

impl From<KeyEvent> for Message {
    fn from(value: KeyEvent) -> Self {
        Self::KeyEvent(value)
    }
}

#[derive(Clone, Debug)]
pub enum StartDbusServiceEvent {
    Started(u8, Connection),
}

impl From<StartDbusServiceEvent> for Message {
    fn from(value: StartDbusServiceEvent) -> Self {
        Self::StartDbusService(value)
    }
}

fn to_modifier_state(keysym: Keysym) -> ModifierState {
    match keysym {
        Keysym::Shift_L | Keysym::Shift_R => ModifierState::Shift,
        Keysym::Caps_Lock => ModifierState::CapsLock,
        Keysym::Control_L | Keysym::Control_R => ModifierState::Ctrl,
        Keysym::Alt_L | Keysym::Alt_R => ModifierState::Alt,
        Keysym::Num_Lock => ModifierState::NumLock,
        Keysym::Super_L | Keysym::Super_R => ModifierState::Super,
        _ => ModifierState::NoState,
    }
}
