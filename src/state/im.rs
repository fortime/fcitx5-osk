use std::{collections::HashMap, rc::Rc, sync::Arc};

use iced::{Font, Task};

use crate::{
    app::Message,
    dbus::{
        client::{Fcitx5ControllerServiceProxy, Fcitx5Services, InputMethodInfo},
        server::CandidateAreaState,
    },
};

#[derive(Default)]
pub struct ImState {
    cur_im: Option<Rc<InputMethodInfo>>,
    ims: HashMap<String, Rc<InputMethodInfo>>,
    font: Font,
    candidate_area_state: Option<Arc<CandidateAreaState>>,
    fcitx5_services: Option<Fcitx5Services>,
}

impl ImState {
    fn reset_candidate_area_state(&mut self) {
        self.candidate_area_state = None;
    }

    fn cur_im_unique_name(&self) -> &str {
        self.cur_im
            .as_ref()
            .map(|im| im.unique_name().as_str())
            .unwrap_or("")
    }

    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    pub fn set_candidate_area_state(&mut self, candidate_area_state: Arc<CandidateAreaState>) {
        self.candidate_area_state = Some(candidate_area_state);
    }

    pub fn update_ims(&mut self, im_list: Vec<InputMethodInfo>) {
        tracing::debug!("New im list: {:?}", im_list);
        self.ims = im_list
            .into_iter()
            .map(|im| (im.unique_name().clone(), Rc::new(im)))
            .collect();
    }

    pub(super) fn update_cur_im(&mut self, unique_name: &str) {
        let cur_im_unique_name = self.cur_im_unique_name();

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
        self.reset_candidate_area_state();
    }

    pub(super) fn update_candidate_font(&mut self, font: Font) {
        self.font = font;
    }

    pub fn candidate_area_state(&self) -> Option<&CandidateAreaState> {
        self.candidate_area_state.as_deref()
    }

    pub fn deactive(&mut self) {
        self.cur_im = None;
        self.reset_candidate_area_state();
    }

    pub fn candidate_font(&self) -> Font {
        self.font
    }
}

// call fcitx5
impl ImState {
    fn fcitx5_controller_service(&self) -> Option<&Fcitx5ControllerServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::controller)
    }

    pub fn sync_input_methods(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_controller_service(),
            format!("get input method group info failed"),
            |s| async move {
                let group_info = s.full_input_method_group_info("").await?;
                Ok(Message::UpdateImList(group_info.into_input_methods()))
            },
        )
    }

    pub fn sync_current_input_method(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_controller_service(),
            format!("get current input method failed"),
            |s| async move {
                let input_method = s.current_input_method().await?;
                Ok(Message::UpdateCurrentIm(input_method))
            },
        )
    }
}
