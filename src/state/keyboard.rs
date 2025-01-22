use std::{collections::HashMap, time::UNIX_EPOCH};

use anyhow::{Context, Result};
use iced::{
    alignment::{Horizontal, Vertical},
    futures::channel::mpsc::UnboundedSender,
    widget::{Button, Column, Text},
    Element, Font, Task,
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
};

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
    id: u8,
    modifiers: u32,
    primary_text_size: u16,
    secondary_text_size: u16,
    font: Font,
    keys: HashMap<String, Key>,
    /// To avoid capturing the lifetime of this object, we seperate the connection process into
    /// multiple steps. In theory, there would be multiple creations of connection in the same
    /// time, we only keep the connection with the correct id only.
    dbus_service_token: u8,
    dbus_service_connection: Option<Connection>,
    fcitx5_services: Option<Fcitx5Services>,
}

impl KeyboardState {
    pub fn new(key_area_layout: &KeyAreaLayout, store: &Store) -> Self {
        let mut res = Self {
            id: 0,
            // always virtual
            modifiers: Default::default(),
            primary_text_size: Default::default(),
            secondary_text_size: Default::default(),
            font: Default::default(),
            keys: HashMap::new(),
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

    pub fn press_key(&mut self, id: u8, key_name: &str, keysym: Keysym) -> Task<Message> {
        // behaviors will be different in differnet VirtualKeyboardFunctionMode
        let mut task = Task::none();

        if id != self.id {
            return task;
        }

        let cur_time = UNIX_EPOCH.elapsed().map(|d| d.as_secs()).unwrap_or(0) as u32;
        let keyval = u32::from(keysym);
        let modifiers = self.modifiers;
        task = task.chain(super::call_fcitx5(
            self.fcitx5_virtual_keyboard_backend_service(),
            format!("send key pressed event failed: {key_name}"),
            |s| async move {
                s.process_key_event(keyval, 0, modifiers, false, cur_time)
                    .await?;
                s.process_key_event(keyval, 0, modifiers, true, cur_time)
                    .await?;
                Ok(Message::Nothing)
            },
        ));

        task
    }

    pub fn release_key(&mut self, id: u8, key_name: &str, keysym: Keysym) -> Task<Message> {
        let mut task = Task::none();

        if id != self.id {
            return task;
        }

        let cur_time = UNIX_EPOCH.elapsed().map(|d| d.as_secs()).unwrap_or(0) as u32;
        let keyval = u32::from(keysym);
        let modifiers = self.modifiers;
        task = task.chain(super::call_fcitx5(
            self.fcitx5_virtual_keyboard_backend_service(),
            format!("send key released event failed: {key_name}"),
            |s| async move {
                s.process_key_event(keyval, 0, modifiers, true, cur_time)
                    .await?;
                Ok(Message::Nothing)
            },
        ));

        task
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
            let (top, middle, keysym) = if is_shift_pressed {
                if let Some(secondary) = key.secondaries().get(0) {
                    (
                        Text::new(""),
                        Text::new(secondary.symbol()),
                        secondary.keysym(),
                    )
                } else {
                    (
                        Text::new(""),
                        Text::new(key.primary().symbol()),
                        key.primary().keysym(),
                    )
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
                    key.primary().keysym(),
                )
            };
            column = column
                .push(
                    top.width(width_p)
                        .height(secondary_height_p)
                        .font(self.font)
                        .size((self.secondary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Right),
                )
                .push(
                    middle
                        .width(width_p)
                        .height(primary_height_p)
                        .font(self.font)
                        .size((self.primary_text_size * unit) as f32)
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            (
                Element::from(column),
                Some(Message::KeyPressed(self.id, key_name.to_string(), *keysym)),
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

#[derive(Clone, Debug)]
pub enum StartDbusServiceEvent {
    Started(u8, Connection),
}

impl From<StartDbusServiceEvent> for Message {
    fn from(value: StartDbusServiceEvent) -> Self {
        Self::StartDbusService(value)
    }
}
