use std::time::Duration;

use iced::{window::Id, Size, Task};
use tokio::time;

use crate::{
    app::Message,
    dbus::client::{Fcitx5Services, Fcitx5VirtualKeyboardServiceProxy},
    window::{WindowManager, WindowSettings},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowStateSnapshot {
    id: Id,
    hide_req_token: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideOpSource {
    Fcitx5,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InnerWindowState {
    Init(Id),
    WmInited(Id),
    Hiding(Id, HideOpSource),
    Hidden,
}

impl InnerWindowState {
    fn id(&self) -> Option<Id> {
        match self {
            InnerWindowState::Init(id) => Some(*id),
            InnerWindowState::WmInited(id) => Some(*id),
            InnerWindowState::Hiding(id, _) => Some(*id),
            InnerWindowState::Hidden => None,
        }
    }
}

pub struct WindowState<WM> {
    state: InnerWindowState,
    hide_req_token: u16,
    fcitx5_services: Option<Fcitx5Services>,
    wm: WM,
}

impl<WM> Default for WindowState<WM>
where
    WM: Default,
{
    fn default() -> Self {
        Self {
            state: InnerWindowState::Hidden,
            hide_req_token: Default::default(),
            fcitx5_services: Default::default(),
            wm: Default::default(),
        }
    }
}

impl<WM> WindowState<WM> {
    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }
}

impl<WM> WindowState<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
{
    fn snapshot(&self) -> Option<WindowStateSnapshot> {
        self.state.id().map(|id| WindowStateSnapshot {
            id,
            hide_req_token: self.hide_req_token,
        })
    }

    fn inc_hide_req_token(&mut self) {
        self.hide_req_token = self.hide_req_token.wrapping_add(1);
    }

    pub fn wm_inited(&self) -> bool {
        if let InnerWindowState::WmInited(_) = self.state {
            true
        } else {
            false
        }
    }

    pub fn set_wm_inited(&mut self, id: Id) {
        if let InnerWindowState::Init(init_id) = self.state {
            if init_id == id {
                self.state = InnerWindowState::WmInited(id);
                return;
            }
        }
        tracing::error!(
            "window is in a wrong state: {:?}, can't update to {:?}",
            self.state,
            InnerWindowState::WmInited(id)
        )
    }

    pub fn show_local(&mut self, settings: WindowSettings) -> Task<WM::Message> {
        if let Some(id) = self.state.id() {
            tracing::warn!("window[{}] is already shown", id);
            // disable all pending hide requests    .
            self.inc_hide_req_token();
            Task::none()
        } else {
            let (id, task) = self.wm.open(settings);
            tracing::debug!("opening window: {}", id);
            self.state = InnerWindowState::Init(id);
            self.hide_req_token = 0;
            task
        }
    }

    pub fn hide_local_with_delay(
        &mut self,
        delay: Duration,
        source: HideOpSource,
    ) -> Task<Message> {
        if let Some(snapshot) = self.snapshot() {
            tracing::debug!("waiting to close window: {:?}", snapshot);
            Task::future(time::sleep(delay))
                .map(move |_| WindowEvent::HideWindow(Some(snapshot), source).into())
        } else {
            tracing::debug!("window is already hidden");
            Task::none()
        }
    }

    pub fn hide_local_checked(
        &mut self,
        last: WindowStateSnapshot,
        source: HideOpSource,
    ) -> Task<WM::Message> {
        let snapshot = self.snapshot();
        if snapshot == Some(last) {
            self.hide_local(source)
        } else {
            tracing::debug!(
                "window state snapshot doesn't match, last: {:?}, current: {:?}",
                snapshot,
                last
            );
            Task::none()
        }
    }

    pub fn hide_local(&mut self, source: HideOpSource) -> Task<WM::Message> {
        match self.state {
            InnerWindowState::Init(id) | InnerWindowState::WmInited(id) => {
                self.state = InnerWindowState::Hiding(id, source);
                tracing::debug!("closing window: {}", id);
                return self.wm.close(id);
            }
            InnerWindowState::Hiding(id, _) => {
                if source == HideOpSource::Fcitx5 {
                    tracing::debug!("update hide op source: {:?}, window: {}", source, id);
                    self.state = InnerWindowState::Hiding(id, source);
                }
            }
            InnerWindowState::Hidden => {
                tracing::debug!("window is already hidden");
            }
        }
        Task::none()
    }

    pub fn set_hidden(&mut self, id: Id) -> Option<HideOpSource> {
        let id_and_source = match self.state {
            InnerWindowState::Hiding(window_id, source) => Some((window_id, source)),
            InnerWindowState::Init(window_id) | InnerWindowState::WmInited(window_id) => {
                // close from external user action
                Some((window_id, HideOpSource::External))
            }
            InnerWindowState::Hidden => None,
        };
        id_and_source.and_then(|(window_id, source)| {
            if window_id == id {
                tracing::debug!("window[{}] closed", window_id);
                self.state = InnerWindowState::Hidden;
                Some(source)
            } else {
                None
            }
        })
    }

    pub fn resize(&mut self, size: Size) -> Task<WM::Message> {
        if let Some(id) = self.state.id() {
            tracing::debug!("resizing window: {}", id);
            self.wm.resize(id, size)
        } else {
            tracing::debug!("window is hidden, don't resize");
            Task::none()
        }
    }
}

// call fcitx5
impl<T> WindowState<T> {
    fn fcitx5_virtual_keyboard_service(
        &self,
    ) -> Option<&Fcitx5VirtualKeyboardServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::virtual_keyboard)
    }

    pub fn _toggle(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send toggle event failed"),
            |s| async move {
                s.toggle_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    pub fn show(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send show event failed"),
            |s| async move {
                s.show_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    pub fn hide(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send hide event failed"),
            |s| async move {
                s.hide_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }
}

#[derive(Clone, Debug)]
pub enum WindowEvent {
    Resize(Id, f32, u16),
    WmInited(Id, Size),
    HideWindow(Option<WindowStateSnapshot>, HideOpSource),
    Hidden(Id),
}

impl From<WindowEvent> for Message {
    fn from(value: WindowEvent) -> Self {
        Self::Window(value)
    }
}
