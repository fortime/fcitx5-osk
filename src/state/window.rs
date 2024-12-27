use std::time::Duration;

use iced::{window::{self, Id, Settings}, Task};
use tokio::time;

use crate::{app::Message, dbus::client::{Fcitx5Services, Fcitx5VirtualKeyboardServiceProxy}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowStateSnapshot {
    id: Id,
    hide_req_token: u16,
}

impl WindowStateSnapshot {}

#[derive(Default)]
pub struct WindowState {
    id: Option<Id>,
    hide_req_token: u16,
    fcitx5_services: Option<Fcitx5Services>,
}

impl WindowState {
    fn snapshot(&self) -> Option<WindowStateSnapshot> {
        self.id.map(|id| WindowStateSnapshot {
            id,
            hide_req_token: self.hide_req_token,
        })
    }

    fn inc_hide_req_token(&mut self) {
        self.hide_req_token = self.hide_req_token.wrapping_add(1);
    }

    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    pub fn show_local(&mut self, settings: Settings) -> Task<Message> {
        if let Some(id) = self.id {
            tracing::warn!("window[{}] is already shown", id);
            // disable all pending hide requests    .
            self.inc_hide_req_token();
            Task::none()
        } else {
            let (id, task) = window::open(settings);
            tracing::debug!("opening window: {}", id);
            self.id = Some(id);
            self.hide_req_token = 0;
            task.then(|_id| Task::none())
        }
    }

    pub fn hide_local_with_delay(&mut self, delay: Duration) -> Task<Message> {
        if let Some(snapshot) = self.snapshot() {
            tracing::debug!("waiting to close window: {:?}", snapshot);
            Task::future(time::sleep(delay)).map(move |_| Message::HideWindow(snapshot))
        } else {
            tracing::debug!("window is already hidden");
            Task::none()
        }
    }

    pub fn hide_local_checked(&mut self, last: WindowStateSnapshot) -> Task<Message> {
        let snapshot = self.snapshot();
        if snapshot.filter(|s| s == &last).is_some() {
            self.hide_local()
        } else {
            tracing::debug!(
                "window state snapshot doesn't match, last: {:?}, current: {:?}",
                snapshot,
                last
            );
            Task::none()
        }
    }

    pub fn hide_local(&mut self) -> Task<Message> {
        if let Some(id) = self.id.take() {
            tracing::debug!("closing window: {}", id);
            window::close(id)
        } else {
            tracing::debug!("window is already hidden");
            Task::none()
        }
    }
}

// call fcitx5
impl WindowState {
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

    pub fn _hide(&self) -> Task<Message> {
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
