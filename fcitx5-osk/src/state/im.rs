use std::{collections::HashMap, rc::Rc, sync::Arc};

use iced::{Font, Task};

use crate::{
    app::{self, Message},
    dbus::{client::InputMethodInfo, server::CandidateAreaState as Fcitx5CandidateAreaState},
    state::KeyboardBackend,
};

pub struct ImState {
    cur_im: Option<Rc<InputMethodInfo>>,
    ims: HashMap<String, Rc<InputMethodInfo>>,
    im_names: Vec<String>,
    candidate_area_state: CandidateAreaState,
    keyboard_backend: KeyboardBackend,
}

impl ImState {
    pub fn new(keyboard_backend: KeyboardBackend) -> Self {
        Self {
            cur_im: Default::default(),
            ims: Default::default(),
            im_names: Default::default(),
            candidate_area_state: Default::default(),
            keyboard_backend,
        }
    }

    fn reset_candidate_cursor(&mut self) {
        // I don't know how to reset the candidate state in fcitx5, so I just reset the cursor.
        self.candidate_area_state.reset_cursor();
    }

    pub fn im_names(&self) -> &[String] {
        &self.im_names
    }

    pub fn im_name(&self) -> Option<&String> {
        self.cur_im.as_ref().map(|im| im.unique_name())
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

    fn deactivate(&mut self, im: &str) {
        if self.cur_im.take_if(|i| i.unique_name() == im).is_some() {
            self.candidate_area_state.reset();
        }
    }

    pub(super) fn on_event(&mut self, event: ImEvent) -> Task<Message> {
        match event {
            ImEvent::UpdateImListAndCurrentIm(ims, im) => {
                self.update_ims(ims);
                self.update_cur_im(&im);
            }
            ImEvent::UpdateCurrentIm(im) => self.update_cur_im(&im),
            ImEvent::SelectIm(im) => return self.select_im(im),
            ImEvent::DeactivateIm(im) => {
                // TODO? other logic
                self.deactivate(&im)
            }
            ImEvent::SyncImList => {
                return self.keyboard_backend.sync_input_methods_and_current_im()
            }
            ImEvent::SyncCurrentIm => return self.sync_current_input_method(),
            ImEvent::ResetCandidateCursor => self.reset_candidate_cursor(),
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
            ImEvent::SelectCandidate(_) => {}
        }
        Message::nothing()
    }
}

// call fcitx5
impl ImState {
    pub(super) fn update_keyboard_backend(&mut self, keyboard_backend: KeyboardBackend) {
        self.keyboard_backend = keyboard_backend;
    }

    fn sync_current_input_method(&self) -> Task<Message> {
        self.keyboard_backend
            .current_input_method()
            .map(|r| match r {
                Ok(input_method) => ImEvent::UpdateCurrentIm(input_method).into(),
                Err(e) => app::error_with_context(e, "get current input method failed"),
            })
    }

    fn select_im(&self, im: String) -> Task<Message> {
        self.keyboard_backend.set_current_im(im).map(|r| match r {
            Ok(_) => Message::Nothing,
            Err(e) => app::error_with_context(e, "select im failed"),
        })
    }

    fn prev_page(&self, page_index: i32) -> Task<Message> {
        self.keyboard_backend
            .prev_page(page_index)
            .map(|r| match r {
                Ok(_) => Message::Nothing,
                Err(e) => app::error_with_context(e, "prev page failed"),
            })
    }

    fn next_page(&self, page_index: i32) -> Task<Message> {
        self.keyboard_backend
            .next_page(page_index)
            .map(|r| match r {
                Ok(_) => Message::Nothing,
                Err(e) => app::error_with_context(e, "next page failed"),
            })
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

    pub fn candidate_list(&self) -> &[Vec<(String, Option<Font>)>] {
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
    UpdateImListAndCurrentIm(Vec<InputMethodInfo>, String),
    UpdateCurrentIm(String),
    SelectIm(String),
    DeactivateIm(String),
    ResetCandidateCursor,
    PrevCandidates,
    NextCandidates(usize),
    SelectCandidate(usize),
}

impl From<ImEvent> for Message {
    fn from(value: ImEvent) -> Self {
        Self::ImEvent(value)
    }
}
