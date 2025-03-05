use std::{collections::HashMap, rc::Rc, sync::Arc};

use iced::Task;

use crate::{
    app::Message,
    dbus::{
        client::{
            Fcitx5ControllerServiceProxy, Fcitx5Services, Fcitx5VirtualKeyboardBackendServiceProxy,
            InputMethodInfo,
        },
        server::CandidateAreaState as Fcitx5CandidateAreaState,
    },
};

#[derive(Default)]
pub struct ImState {
    cur_im: Option<Rc<InputMethodInfo>>,
    ims: HashMap<String, Rc<InputMethodInfo>>,
    im_names: Vec<String>,
    candidate_area_state: CandidateAreaState,
    fcitx5_services: Option<Fcitx5Services>,
}

impl ImState {
    pub fn reset_candidate_cursor(&mut self) {
        // I don't know how to reset the candidate state in fcitx5, so I just reset the cursor.
        self.candidate_area_state.reset_cursor();
    }

    pub fn im_names(&self) -> &[String] {
        &self.im_names
    }

    pub fn im_name(&self) -> Option<&String> {
        self.cur_im.as_ref().map(|im| im.unique_name())
    }

    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    pub fn update_candidate_area_state(&mut self, state: Arc<Fcitx5CandidateAreaState>) {
        self.candidate_area_state.update(state);
    }

    fn update_ims(&mut self, ims: Vec<InputMethodInfo>) {
        tracing::debug!("New im list: {:?}", ims);
        self.ims = ims
            .into_iter()
            .map(|im| (im.unique_name().clone(), Rc::new(im)))
            .collect();
        self.im_names = self.ims.keys().cloned().collect();
    }

    pub(super) fn update_cur_im(&mut self, unique_name: &str) {
        let cur_im_unique_name = self.im_name().map(|n| n.as_str()).unwrap_or("");

        tracing::debug!(
            "change current im[{}] from im[{}]",
            unique_name,
            cur_im_unique_name
        );

        if unique_name == cur_im_unique_name {
            return;
        }

        self.cur_im = self.ims.get(unique_name).cloned();
        if self.cur_im.is_none() {
            tracing::warn!("unable to find im: {}", unique_name);
        }
        self.candidate_area_state.reset();
    }

    pub fn candidate_area_state(&self) -> &CandidateAreaState {
        &self.candidate_area_state
    }

    fn deactive(&mut self, im: &str) {
        if self.cur_im.take_if(|i| i.unique_name() == im).is_some() {
            self.candidate_area_state.reset();
        }
    }

    pub(super) fn on_event(&mut self, event: ImEvent) -> Task<Message> {
        match event {
            ImEvent::UpdateImList(ims) => self.update_ims(ims),
            ImEvent::UpdateCurrentIm(im) => self.update_cur_im(&im),
            ImEvent::SelectIm(im) => return self.select_im(im),
            ImEvent::DeactivateIm(im) => {
                // TODO? other logic
                self.deactive(&im)
            }
            ImEvent::SyncImList => return self.sync_input_methods(),
            ImEvent::SyncCurrentIm => return self.sync_current_input_method(),
            ImEvent::PrevCandidates => {
                if let Some(page_index) = self.candidate_area_state.prev() {
                    return self.prev_page(page_index);
                }
            }
            ImEvent::NextCandidates(c) => {
                if let Some(page_index) = self.candidate_area_state.next(c) {
                    return self.next_page(page_index);
                }
            }
            ImEvent::SelectCandidate(c) => return self.select_candidate(c),
        }
        Task::none()
    }
}

// call fcitx5
impl ImState {
    fn fcitx5_controller_service(&self) -> Option<&Fcitx5ControllerServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::controller)
    }

    fn fcitx5_virtual_keyboard_backend_service(
        &self,
    ) -> Option<&Fcitx5VirtualKeyboardBackendServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::virtual_keyboard_backend)
    }

    fn sync_input_methods(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_controller_service(),
            format!("get input method group info failed"),
            |s| async move {
                let group_info = s.full_input_method_group_info("").await?;
                Ok(ImEvent::UpdateImList(group_info.into_input_methods()).into())
            },
        )
    }

    fn sync_current_input_method(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_controller_service(),
            format!("get current input method failed"),
            |s| async move {
                let input_method = s.current_input_method().await?;
                Ok(ImEvent::UpdateCurrentIm(input_method).into())
            },
        )
    }

    fn select_im(&self, im: String) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_controller_service(),
            format!("select im failed"),
            |s| async move {
                s.set_current_im(&im).await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn select_candidate(&self, cursor: usize) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_backend_service(),
            format!("select candidate {} failed", cursor),
            |s| async move {
                // TODO 0-based or 1-based
                s.select_candidate(cursor as i32).await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn prev_page(&self, page_index: i32) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_backend_service(),
            format!("prev page failed"),
            |s| async move {
                s.prev_page(page_index).await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn next_page(&self, page_index: i32) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_backend_service(),
            format!("next page failed"),
            |s| async move {
                s.next_page(page_index).await?;
                Ok(Message::Nothing)
            },
        )
    }
}

#[derive(Default)]
pub struct CandidateAreaState {
    fcitx5_state: Option<Arc<Fcitx5CandidateAreaState>>,
    /// if candidate list is paged in fcitx5
    paged: bool,
    prev_cursors: Vec<usize>,
    cursor: usize,
}

impl CandidateAreaState {
    pub fn update(&mut self, state: Arc<Fcitx5CandidateAreaState>) {
        self.reset_cursor();
        self.paged = false;
        if !state.candidate_text_list().is_empty() {
            if state.has_prev() || state.has_next() {
                self.paged = true;
            }
            self.fcitx5_state = Some(state);
        } else {
            self.fcitx5_state = None;
        }
    }

    /// Be careful, this function should only be called, when the candidate state has been reset in
    /// fcitx5.
    fn reset(&mut self) {
        self.reset_cursor();
        self.fcitx5_state = None;
        self.paged = false;
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
        self.prev_cursors.clear();
        if self.prev_cursors.capacity() > 16 {
            self.prev_cursors.shrink_to(16);
        }
    }

    pub fn next(&mut self, cursor: usize) -> Option<i32> {
        if let Some(fcitx5_state) = &self.fcitx5_state {
            if cursor > self.cursor && cursor < fcitx5_state.candidate_text_list().len() {
                self.prev_cursors.push(self.cursor);
                self.cursor = cursor;
            } else if cursor >= fcitx5_state.candidate_text_list().len() && fcitx5_state.has_next()
            {
                return Some(fcitx5_state.page_index());
            }
        }
        None
    }

    pub fn prev(&mut self) -> Option<i32> {
        if !self.prev_cursors.is_empty() {
            self.cursor = self.prev_cursors.pop().unwrap_or(0);
        } else if self.cursor != 0 {
            self.cursor = 0;
        } else {
            // check if there is any previous page in fcitx5
            if let Some(fcitx5_state) = &self.fcitx5_state {
                if fcitx5_state.has_prev() {
                    return Some(fcitx5_state.page_index() - 1);
                }
            }
        }
        None
    }

    pub fn has_prev_in_fcitx5(&self) -> bool {
        self.fcitx5_state
            .as_ref()
            .map(|s| s.has_prev())
            .unwrap_or(false)
    }

    pub fn has_next_in_fcitx5(&self) -> bool {
        self.fcitx5_state
            .as_ref()
            .map(|s| s.has_next())
            .unwrap_or(false)
    }

    pub fn is_paged(&self) -> bool {
        self.paged
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn has_candidate(&self) -> bool {
        self.fcitx5_state.is_some()
    }

    pub fn candidate_list(&self) -> &[String] {
        self.fcitx5_state
            .as_ref()
            .map(|s| &s.candidate_text_list()[self.cursor..])
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub enum ImEvent {
    SyncImList,
    SyncCurrentIm,
    UpdateImList(Vec<InputMethodInfo>),
    UpdateCurrentIm(String),
    SelectIm(String),
    DeactivateIm(String),
    PrevCandidates,
    NextCandidates(usize),
    SelectCandidate(usize),
}

impl From<ImEvent> for Message {
    fn from(value: ImEvent) -> Self {
        Self::ImEvent(value)
    }
}
