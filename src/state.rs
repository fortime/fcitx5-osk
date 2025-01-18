use std::{future::Future, rc::Rc, sync::Arc};

use anyhow::{Error, Result};
use getset::{Getters, MutGetters};
use iced::Task;
use zbus::Result as ZbusResult;

use crate::{
    app::{KeyboardError, Message},
    dbus::client::Fcitx5Services,
    layout::KeyAreaLayout,
    store::Store,
};

mod im;
mod keyboard;
mod layout;
mod window;

pub use im::ImState;
pub use keyboard::{KeyboardState, ModifierState, StartDbusServiceEvent};
pub use layout::LayoutState;
pub use window::{HideOpSource, WindowEvent, WindowState, WindowStateSnapshot};

#[derive(Getters, MutGetters)]
pub struct State<WM> {
    #[getset(get = "pub", get_mut = "pub")]
    layout: LayoutState,
    #[getset(get = "pub", get_mut = "pub")]
    keyboard: KeyboardState,
    #[getset(get = "pub", get_mut = "pub")]
    im: ImState,
    #[getset(get = "pub", get_mut = "pub")]
    window: WindowState<WM>,
    has_fcitx5_services: bool,
}

impl<WM> State<WM>
where
    WM: Default,
{
    pub fn new(keyboard: KeyboardState, layout: LayoutState) -> Self {
        Self {
            layout,
            keyboard,
            im: Default::default(),
            window: Default::default(),
            has_fcitx5_services: false,
        }
    }
}

impl<WM> State<WM> {
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
        if self.has_fcitx5_services {
            Task::none()
        } else {
            Task::perform(Fcitx5Services::new(), |res: ZbusResult<_>| match res {
                Ok(services) => StartedEvent::StartedDbusClients(services).into(),
                Err(e) => fatal_with_context(e, "failed to create dbus clients"),
            })
        }
    }

    pub fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) -> bool {
        if self.has_fcitx5_services {
            false
        } else {
            self.keyboard.set_dbus_clients(fcitx5_services.clone());
            self.im.set_dbus_clients(fcitx5_services.clone());
            self.window.set_dbus_clients(fcitx5_services.clone());
            self.has_fcitx5_services = true;
            true
        }
    }
}

#[derive(Clone, Debug)]
pub enum StartedEvent {
    StartedDbusClients(Fcitx5Services),
}

impl From<StartedEvent> for Message {
    fn from(value: StartedEvent) -> Self {
        Self::Started(value)
    }
}

fn call_fcitx5<S, M, FN, F>(service: Option<&S>, err_msg: M, f: FN) -> Task<Message>
where
    S: Clone,
    M: Into<String>,
    FN: FnOnce(S) -> F,
    F: Future<Output = Result<Message>> + 'static + Send,
{
    let err_msg = err_msg.into();
    if let Some(service) = service {
        let service = service.clone();
        Task::perform(f(service), move |r| match r {
            Err(e) => error_with_context(e, err_msg.clone()),
            Ok(t) => t,
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
