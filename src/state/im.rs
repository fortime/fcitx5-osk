use std::{collections::HashMap, rc::Rc};

use iced::Task;

use crate::{app::Message, dbus::client::{Fcitx5ControllerServiceProxy, Fcitx5Services, InputMethodInfo}};

#[derive(Default)]
pub struct ImState {
    cur_im: Option<Rc<InputMethodInfo>>,
    ims: HashMap<String, Rc<InputMethodInfo>>,
    fcitx5_services: Option<Fcitx5Services>,
}

impl ImState {
    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    pub fn update_ims(&mut self, im_list: Vec<InputMethodInfo>) {
        tracing::debug!("New im list: {:?}", im_list);
        self.ims = im_list
            .into_iter()
            .map(|im| (im.unique_name().clone(), Rc::new(im)))
            .collect();
    }

    pub fn update_cur_im(&mut self, unique_name: &str) {
        tracing::debug!("current im: {}", unique_name);
        self.cur_im = self.ims.get(unique_name).cloned();
        if self.cur_im.is_none() {
            tracing::warn!("unable to find im: {}", unique_name);
        }
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
