use std::{mem, rc::Rc};

use iced::{
    alignment::Horizontal,
    widget::{Column, Container},
    Element, Font, Padding, Size,
};

use crate::{
    app::Message,
    layout::{KLength, KeyAreaLayout, SettingLayout, ToElementCommonParams, ToolbarLayout},
};

pub struct LayoutState {
    size: (KLength, KLength),
    scale_factor: f32,
    unit: KLength,
    //fit: bool,
    padding: Padding,
    toolbar_layout: ToolbarLayout,
    candidate_font: Font,
    key_area_layout: Rc<KeyAreaLayout>,
    setting_layout: SettingLayout,
    setting_shown: bool,
    width: KLength,
}

impl LayoutState {
    pub fn new(width: KLength, key_area_layout: Rc<KeyAreaLayout>) -> Self {
        let mut res = Self {
            size: Default::default(),
            scale_factor: 1.0,
            unit: Default::default(),
            padding: Default::default(),
            toolbar_layout: ToolbarLayout::new(key_area_layout.min_toolbar_height_u()),
            candidate_font: Default::default(),
            key_area_layout,
            setting_layout: SettingLayout,
            setting_shown: false,
            width,
        };
        res.calculate_size();
        res
    }

    fn unit_within(&self, width: KLength) -> KLength {
        // plus two units of padding
        let width_u = self.key_area_layout.width_u() + 2;

        width / width_u
    }

    fn calculate_size(&mut self) {
        // because of scaling issue, the actual window size is different from the one calculated in
        // this method.
        let unit = self.unit_within(self.width);
        let key_area_size = self.key_area_layout.size(unit);

        self.unit = unit;
        let width = key_area_size.0 + 2 * unit;
        // one padding is between toolbar and key_area, two paddings are of the keyboard.
        let height = key_area_size.1 + (self.toolbar_layout.height_u() + 3) * unit;
        self.size = (width, height);
        self.padding = Padding::from([unit.val(), unit.val()]);
        tracing::debug!(
            "unit: {}, keyboard size: {:?}, key area size: {:?}, padding: {:?}",
            self.unit,
            self.size,
            key_area_size,
            self.padding
        );
    }

    pub fn available_candidate_width(&self) -> KLength {
        // minus padding
        self.size.0 - 2 * self.unit
    }

    pub fn size(&self) -> Size<KLength> {
        Size::from(self.size)
    }

    pub fn unit(&self) -> KLength {
        self.unit
    }

    pub fn font_size(&self) -> KLength {
        self.unit * self.key_area_layout.primary_text_size_u()
    }

    pub fn update_width(&mut self, width: KLength) {
        self.width = width;
        self.calculate_size();
    }

    pub fn update_scale_factor(&mut self, mut scale_factor: f32) -> f32 {
        mem::swap(&mut self.scale_factor, &mut scale_factor);
        self.calculate_size();
        scale_factor
    }

    pub fn update_key_area_layout(
        &mut self,
        mut width: KLength,
        mut key_area_layout: Rc<KeyAreaLayout>,
    ) -> Rc<KeyAreaLayout> {
        let new_min_toolbar_height_u = key_area_layout.min_toolbar_height_u();
        mem::swap(&mut self.key_area_layout, &mut key_area_layout);
        mem::swap(&mut self.width, &mut width);
        self.toolbar_layout
            .update_height_u(new_min_toolbar_height_u);
        self.calculate_size();
        key_area_layout
    }

    pub fn update_candidate_font(&mut self, font: Font) {
        self.candidate_font = font;
    }

    pub fn is_setting_shown(&self) -> bool {
        self.setting_shown
    }

    pub fn to_element<'a, 'b>(
        &'a self,
        params: &'a ToElementCommonParams<'b>,
    ) -> Element<'b, Message> {
        let size = self.size();
        let mut keyboard = Column::new()
            .align_x(Horizontal::Center)
            .width(size.width)
            .height(size.height)
            .padding(self.padding)
            .spacing(self.unit)
            .push(self.toolbar_layout.to_element(
                params,
                self.unit,
                self.candidate_font,
                self.key_area_layout.primary_text_size_u(),
            ));

        keyboard = if self.setting_shown {
            keyboard.push(
                Container::new(self.setting_layout.to_element(
                    params,
                    self.unit,
                    self.key_area_layout.primary_text_size_u(),
                ))
                .height(self.key_area_layout.height_u() * self.unit),
            )
        } else {
            keyboard.push(self.key_area_layout.to_element(self.unit, params.state))
        };
        keyboard.into()
    }

    pub fn on_event(&mut self, event: LayoutEvent) {
        match event {
            LayoutEvent::ToggleSetting => self.setting_shown = !self.setting_shown,
            LayoutEvent::SyncLayout => {}
        }
    }
}

#[derive(Clone, Debug)]
pub enum LayoutEvent {
    SyncLayout,
    ToggleSetting,
}

impl From<LayoutEvent> for Message {
    fn from(value: LayoutEvent) -> Self {
        Self::LayoutEvent(value)
    }
}
