use std::{collections::HashMap, future::Future, mem, rc::Rc, sync::Arc, time::UNIX_EPOCH};

use anyhow::{Context, Error, Result};
use iced::{
    alignment::{Horizontal, Vertical},
    futures::channel::mpsc::UnboundedSender,
    widget::{Button, Column, Text},
    Element, Padding, Size, Task,
};
use xkeysym::Keysym;
use zbus::{Connection, Result as ZbusResult};

use crate::{
    dbus::{
        client::{Fcitx5VirtualKeyboardBackendServiceProxy, Fcitx5VirtualKeyboardServiceProxy},
        server::Fcitx5VirtualkeyboardImPanelService,
    },
    key_set::Key,
    layout::{KeyAreaLayout, KeyManager},
    store::Store,
};

use super::{KeyboardError, Message};

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
        self.keyboard
            .update_key_area_layout(&key_area_layout, store);
        self.layout.update_key_area_layout(key_area_layout)
    }

    pub fn start(&mut self) -> Task<Message> {
        self.keyboard.start()
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
        let mut width_p = self.size_p.0;
        // when width or height mod 4 = 1, the size of this layout is not the same as the size of
        // window. So, when width or height mod 4 = 1, width or height will be increased by 1.
        if width_p % 4 == 1 {
            width_p += 1;
        }
        if width_p < Self::MIN_P {
            anyhow::bail!("width is too small: {}", width_p);
        }
        let trimmed_width_p = width_p - Self::MIN_PADDING_P * 2;
        let unit = self.key_area_layout.unit_within(trimmed_width_p);
        let key_area_size_p = self.key_area_layout.size_p(unit);

        self.unit = unit;
        let height_p_without_padding = key_area_size_p.1 + 4 + (Self::TOOLBAR_HEIGHT + 1) * unit;
        let mut height_p = height_p_without_padding + Self::MIN_PADDING_P * 2;
        if height_p % 4 == 1 {
            height_p += 1;
        }
        self.size_p = (width_p, height_p);
        self.padding = Padding::from([
            (height_p - height_p_without_padding) as f32 / 2.0,
            (width_p - key_area_size_p.0) as f32 / 2.0,
        ]);
        tracing::debug!(
            "unit: {}, keyboard size: {:?}, key area size: {:?} padding: {:?}",
            self.unit,
            self.size_p,
            key_area_size_p,
            self.padding
        );
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
        let size = self.size();
        Column::new()
            .align_x(Horizontal::Center)
            .width(size.width)
            .height(size.height)
            .padding(self.padding)
            .spacing(self.unit)
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
    id: u8,
    input: String,
    modifiers: u32,
    primary_text_size: u16,
    secondary_text_size: u16,
    keys: HashMap<String, Key>,
    /// To avoid capturing the lifetime of this object, we seperate the connection process into
    /// multiple steps. In theory, there would be multiple creations of connection in the same
    /// time, we only keep the connection with the collect id only.
    dbus_service_id: u8,
    dbus_service_connection: Option<Connection>,
    fcitx5_virtual_keyboard_service: Option<Fcitx5VirtualKeyboardServiceProxy<'static>>,
    fcitx5_virtual_keyboard_backend_service:
        Option<Fcitx5VirtualKeyboardBackendServiceProxy<'static>>,
}

impl KeyboardState {
    pub fn new(key_area_layout: &KeyAreaLayout, store: &Store) -> Self {
        let mut res = Self {
            id: 0,
            input: String::new(),
            // always virtual
            modifiers: Default::default(),
            primary_text_size: Default::default(),
            secondary_text_size: Default::default(),
            keys: HashMap::new(),
            dbus_service_id: 0,
            dbus_service_connection: None,
            fcitx5_virtual_keyboard_service: None,
            fcitx5_virtual_keyboard_backend_service: None,
        };
        res.update_key_area_layout(key_area_layout, store);
        res
    }

    pub fn update_key_area_layout(&mut self, key_area_layout: &KeyAreaLayout, store: &Store) {
        self.id = self.id.wrapping_add(1);
        self.modifiers = ModifierState::Virtual as u32;
        self.primary_text_size = key_area_layout.primary_text_size();
        self.secondary_text_size = key_area_layout.secondary_text_size();
        self.keys = key_area_layout
            .key_mappings()
            .iter()
            .filter_map(|(k, v)| store.key(v).map(|key| (k.clone(), key.clone())))
            .collect();
    }

    pub fn show_local(&mut self) -> Task<Message> {
        todo!()
    }

    pub fn hide_local(&mut self) -> Task<Message> {
        todo!()
    }

    pub fn update_input(&mut self, s: String) {
        self.input.push_str(&s);
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn start(&mut self) -> Task<Message> {
        if self.fcitx5_virtual_keyboard_service.is_some()
            && self.fcitx5_virtual_keyboard_backend_service.is_some()
        {
            Task::none()
        } else {
            Task::perform(
                async {
                    let connection = Connection::session().await?;
                    let s1 = Fcitx5VirtualKeyboardServiceProxy::new(&connection).await?;
                    let s2 = Fcitx5VirtualKeyboardBackendServiceProxy::new(&connection).await?;
                    Ok((s1, s2))
                },
                |res: ZbusResult<_>| match res {
                    Ok((s1, s2)) => StartedState::StartedDbusClients(s1, s2).into(),
                    Err(e) => fatal_with_context(e, "failed to create dbus clients"),
                },
            )
        }
    }

    pub fn start_dbus_service(&mut self, tx: UnboundedSender<Message>) -> Task<Message> {
        // drop the old one
        let _ = self.dbus_service_connection.take();

        self.dbus_service_id = self.dbus_service_id.wrapping_add(1);

        let new_dbus_service_id = self.dbus_service_id;
        Task::perform(
            async move {
                tracing::debug!("start dbus service: {}", new_dbus_service_id);
                let s = Fcitx5VirtualkeyboardImPanelService::new(tx);
                let connection = s.start().await.context("failed to start dbus service")?;
                Ok((new_dbus_service_id, connection))
            },
            |res: Result<_>| match res {
                Ok((id, connection)) => StartDbusServiceState::Started(id, connection).into(),
                Err(e) => fatal(e),
            },
        )
    }

    pub fn set_dbus_service_connection(
        &mut self,
        dbus_service_id: u8,
        connection: Connection,
    ) -> bool {
        if dbus_service_id != self.dbus_service_id {
            tracing::warn!(
                "concurrency creation of dbus service, connection is dropped: {}/{:?}",
                dbus_service_id,
                connection
            );
            false
        } else {
            self.dbus_service_connection = Some(connection);
            true
        }
    }

    pub fn set_dbus_clients(
        &mut self,
        fcitx5_virtual_keyboard_service: Fcitx5VirtualKeyboardServiceProxy<'static>,
        fcitx5_virtual_keyboard_backend_service: Fcitx5VirtualKeyboardBackendServiceProxy<'static>,
    ) -> bool {
        if self.fcitx5_virtual_keyboard_service.is_some() {
            false
        } else {
            self.fcitx5_virtual_keyboard_service = Some(fcitx5_virtual_keyboard_service);
            self.fcitx5_virtual_keyboard_backend_service =
                Some(fcitx5_virtual_keyboard_backend_service);
            true
        }
    }
}

// call fcitx5
impl KeyboardState {
    pub fn _toggle(&mut self) -> Task<Message> {
        call_fcitx5(
            &self.fcitx5_virtual_keyboard_service,
            format!("send toggle event failed"),
            |s| async move { s.toggle_virtual_keyboard().await },
        )
    }

    pub fn show(&mut self) -> Task<Message> {
        call_fcitx5(
            &self.fcitx5_virtual_keyboard_service,
            format!("send show event failed"),
            |s| async move { s.show_virtual_keyboard().await },
        )
    }

    pub fn _hide(&mut self) -> Task<Message> {
        call_fcitx5(
            &self.fcitx5_virtual_keyboard_service,
            format!("send hide event failed"),
            |s| async move { s.hide_virtual_keyboard().await },
        )
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
        task = task.chain(call_fcitx5(
            &self.fcitx5_virtual_keyboard_backend_service,
            format!("send key pressed event failed: {key_name}"),
            |s| async move {
                s.process_key_event(keyval, 0, modifiers, true, cur_time)
                    .await
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
        task = task.chain(call_fcitx5(
            &self.fcitx5_virtual_keyboard_backend_service,
            format!("send key released event failed: {key_name}"),
            |s| async move {
                s.process_key_event(keyval, 0, modifiers, true, cur_time)
                    .await
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
pub enum StartDbusServiceState {
    New(UnboundedSender<Message>),
    Started(u8, Connection),
}

impl From<StartDbusServiceState> for Message {
    fn from(value: StartDbusServiceState) -> Self {
        Self::StartDbusService(value)
    }
}

#[derive(Clone, Debug)]
pub enum StartedState {
    StartedDbusClients(
        Fcitx5VirtualKeyboardServiceProxy<'static>,
        Fcitx5VirtualKeyboardBackendServiceProxy<'static>,
    ),
}

impl From<StartedState> for Message {
    fn from(value: StartedState) -> Self {
        Self::Started(value)
    }
}

fn call_fcitx5<S, M, FN, F>(service: &Option<S>, err_msg: M, f: FN) -> Task<Message>
where
    S: Clone,
    M: Into<String>,
    FN: FnOnce(S) -> F,
    F: Future<Output = ZbusResult<()>> + 'static + Send,
{
    let err_msg = err_msg.into();
    let service = service.clone();
    if let Some(service) = service {
        Task::perform(f(service), move |r| {
            if let Err(e) = r {
                error_with_context(e, err_msg.clone())
            } else {
                Message::Nothing
            }
        })
    } else {
        Task::done(fatal(anyhow::anyhow!(
            "dbus client hasn't been initialized"
        )))
    }
}

fn _error<E>(e: E) -> Message
where
    E: Into<Error>,
{
    KeyboardError::Error(Arc::new(e.into())).into()
}

fn error_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Error(Arc::new(e.into().context(err_msg.into()))).into()
}

fn fatal<E>(e: E) -> Message
where
    E: Into<Error>,
{
    KeyboardError::Fatal(Arc::new(e.into())).into()
}

fn fatal_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Fatal(Arc::new(e.into().context(err_msg.into()))).into()
}
