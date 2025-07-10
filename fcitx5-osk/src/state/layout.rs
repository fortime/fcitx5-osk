use std::{mem, rc::Rc, result::Result as StdResult};

use anyhow::Result;
use iced::{
    alignment::Horizontal,
    widget::{self, Column, Container},
    Element, Font, Padding, Size,
};

use crate::{
    app::Message,
    layout::{KeyAreaLayout, SettingLayout, ToElementCommonParams, ToolbarLayout},
};

pub struct LayoutState {
    size: (u16, u16),
    scale_factor: f32,
    unit: u16,
    //fit: bool,
    padding: Padding,
    toolbar_layout: ToolbarLayout,
    candidate_font: Font,
    key_area_layout: Rc<KeyAreaLayout>,
    setting_layout: SettingLayout,
    setting_shown: bool,
    max_width: u16,
}

impl LayoutState {
    pub fn new(width: u16, key_area_layout: Rc<KeyAreaLayout>) -> Result<Self> {
        let mut res = Self {
            size: (0, 0),
            scale_factor: 1.0,
            unit: Default::default(),
            padding: Default::default(),
            toolbar_layout: ToolbarLayout::new(key_area_layout.min_toolbar_height_u()),
            candidate_font: Default::default(),
            key_area_layout,
            setting_layout: SettingLayout,
            setting_shown: false,
            max_width: width,
        };
        res.calculate_size()?;
        Ok(res)
    }

    pub fn unit_within(&self, width: u16) -> u16 {
        // plus two units of padding
        let width_u = self.key_area_layout.width_u() + 2;

        let mut step = 1;
        loop {
            if (self.scale_factor * step as f32).fract() == 0.0 {
                break;
            }
            step += 1;
        }

        let mut unit = step;
        while unit * width_u <= width {
            unit += step;
        }

        // make sure unit has changed
        if unit > step {
            // the last valid unit
            unit -= step;
        }

        unit
    }

    fn calculate_size(&mut self) -> Result<()> {
        // because of scaling issue, the actual window size is different from the one calculated in
        // this method.
        let unit = self.unit_within(self.max_width);
        let key_area_size = self.key_area_layout.size(unit);

        self.unit = unit;
        let width = key_area_size.0 + 2 * unit;
        // one padding is between toolbar and key_area, two paddings are of the keyboard.
        let height = key_area_size.1 + (self.toolbar_layout.height_u() + 3) * unit;
        self.size = (width, height);
        self.padding = Padding::from([(2 * unit) as f32 / 2.0, (2 * unit) as f32 / 2.0]);
        tracing::debug!(
            "unit: {}, keyboard size: {:?}, key area size: {:?} padding: {:?}",
            self.unit,
            self.size,
            key_area_size,
            self.padding
        );
        Ok(())
    }

    pub fn available_candidate_width(&self) -> u16 {
        // minus padding
        self.size.0 - 2 * self.unit
    }

    pub fn size(&self) -> Size {
        Size::from((self.size.0 as f32, self.size.1 as f32))
    }

    pub fn max_width(&self) -> u16 {
        self.max_width
    }

    pub fn unit(&self) -> u16 {
        self.unit
    }

    pub fn update_unit(&mut self, unit: u16, max_width: u16) -> StdResult<u16, u16> {
        let old_unit = self.unit;
        let mut width = self.size.0 / self.unit * unit;
        if width > max_width {
            return Err(unit);
        }
        mem::swap(&mut self.max_width, &mut width);
        if let Err(e) = self.calculate_size() {
            tracing::warn!("failed to update width: {e}, recovering.");
            // recover
            mem::swap(&mut self.max_width, &mut width);
            Err(unit)
        } else {
            Ok(old_unit)
        }
    }

    pub fn update_scale_factor(&mut self, mut scale_factor: f32) -> StdResult<f32, f32> {
        mem::swap(&mut self.scale_factor, &mut scale_factor);
        if let Err(e) = self.calculate_size() {
            tracing::warn!("failed to update scale factor: {e}, recovering.");
            // recover
            mem::swap(&mut self.scale_factor, &mut scale_factor);
            Err(scale_factor)
        } else {
            Ok(scale_factor)
        }
    }

    pub fn update_key_area_layout(
        &mut self,
        mut max_width: u16,
        mut key_area_layout: Rc<KeyAreaLayout>,
    ) -> StdResult<Rc<KeyAreaLayout>, Rc<KeyAreaLayout>> {
        let old_min_toolbar_height_u = self.key_area_layout.min_toolbar_height_u();
        let new_min_toolbar_height_u = key_area_layout.min_toolbar_height_u();
        mem::swap(&mut self.key_area_layout, &mut key_area_layout);
        mem::swap(&mut self.max_width, &mut max_width);
        self.toolbar_layout
            .update_height_u(new_min_toolbar_height_u);
        if let Err(e) = self.calculate_size() {
            tracing::warn!(
                "failed to update key area layout[{}]: {e}, recovering.",
                key_area_layout.name()
            );
            // recover
            mem::swap(&mut self.key_area_layout, &mut key_area_layout);
            mem::swap(&mut self.max_width, &mut max_width);
            self.toolbar_layout
                .update_height_u(old_min_toolbar_height_u);
            Err(key_area_layout)
        } else {
            Ok(key_area_layout)
        }
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
        let state = params.state;
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
        // we let keyboard in a stack even there is no overlay, so the widget tree always has the
        // same level. Otherwise, the state will be clear if the level is changed.
        let mut stack = widget::stack![keyboard];
        stack = stack.push_maybe(state.keyboard().popup_overlay(self.unit, self.size));
        stack.into()
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
