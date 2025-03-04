use std::{mem, rc::Rc};

use anyhow::Result;
use iced::{
    alignment::Horizontal,
    widget::{self, Column},
    Element, Font, Padding, Size, Theme,
};

use crate::{
    app::Message,
    layout::{KeyAreaLayout, KeyManager, KeyboardManager, ToolbarLayout},
    state::im::CandidateAreaState,
};

pub struct LayoutState {
    size_p: (u16, u16),
    scale_factor: f32,
    unit: u16,
    //fit: bool,
    padding: Padding,
    toolbar_layout: ToolbarLayout,
    candidate_font: Font,
    key_area_layout: Rc<KeyAreaLayout>,
    show_setting: bool,
}

impl LayoutState {
    const MIN_P: u16 = 640;

    pub fn new(width_p: u16, key_area_layout: Rc<KeyAreaLayout>) -> Result<Self> {
        let mut res = Self {
            size_p: (width_p, 0),
            scale_factor: 1.0,
            unit: Default::default(),
            padding: Default::default(),
            toolbar_layout: ToolbarLayout::new(),
            candidate_font: Default::default(),
            key_area_layout,
            show_setting: false,
        };
        res.calculate_size()?;
        Ok(res)
    }

    fn unit_within(&self, width_p: u16) -> u16 {
        // plus two units of padding
        let width = self.key_area_layout.width() + 2;

        let mut unit = width_p / width;
        if unit < 1 {
            tracing::warn!("width: {width_p} are too small");
            unit = 1;
        }

        while (unit as f32 * self.scale_factor).fract() != 0.0 {
            tracing::warn!(
                "physical size of unit has fraction, increase it: {} / {}",
                unit,
                self.scale_factor
            );
            unit += 1;
        }

        unit
    }

    fn calculate_size(&mut self) -> Result<()> {
        // because of scaling issue, the actual window size is different from the one calculated in
        // this method.
        let width_p = self.size_p.0;
        if width_p < Self::MIN_P {
            anyhow::bail!("width is too small: {}", width_p);
        }
        let unit = self.unit_within(width_p);
        let key_area_size_p = self.key_area_layout.size_p(unit);

        self.unit = unit;
        let width_p = key_area_size_p.0 + 2 * unit;
        // one padding is between toolbar and key_area, two paddings are of the keyboard.
        let height_p = key_area_size_p.1 + (self.toolbar_layout.height() + 3) * unit;
        self.size_p = (width_p, height_p);
        self.padding = Padding::from([(2 * unit) as f32 / 2.0, (2 * unit) as f32 / 2.0]);
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

    pub fn update_width(&mut self, mut width_p: u16, scale_factor: f32) -> bool {
        mem::swap(&mut self.size_p.0, &mut width_p);
        self.scale_factor = scale_factor;
        if let Err(e) = self.calculate_size() {
            tracing::debug!("failed to update width: {e}, recovering.");
            // recover
            mem::swap(&mut self.size_p.0, &mut width_p);
            false
        } else {
            true
        }
    }

    pub(super) fn update_key_area_layout(
        &mut self,
        mut key_area_layout: Rc<KeyAreaLayout>,
    ) -> bool {
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

    pub(super) fn update_candidate_font(&mut self, font: Font) {
        self.candidate_font = font;
    }

    pub fn to_element<'a, 'b, KbdM, KM, M>(
        &'a self,
        candidate_area_state: &'b CandidateAreaState,
        keyboard_manager: &'b KbdM,
        key_manager: &'b KM,
        theme: &'a Theme,
    ) -> Element<'b, M>
    where
        KbdM: KeyboardManager<Message = M>,
        KM: KeyManager<Message = M>,
        M: 'static + Clone,
    {
        //let mut candidates: String = candidate_area_state
        //    .into_iter()
        //    .flat_map(|s| s.candidate_text_list().iter().enumerate())
        //    .map(|(pos, text)| format!("{}. {} | ", pos + 1, text))
        //    .collect();
        // candidates = format!("候选：{}", candidates);
        let size = self.size();
        let keyboard = Column::new()
            .align_x(Horizontal::Center)
            .width(size.width)
            .height(size.height)
            .padding(self.padding)
            .spacing(self.unit)
            .push(self.toolbar_layout.to_element(
                keyboard_manager,
                self.unit,
                candidate_area_state,
                self.candidate_font,
                self.key_area_layout.primary_text_size(),
                theme,
            ))
            .push(self.key_area_layout.to_element(self.unit, key_manager));
        // we let keyboard in a stack even there is no overlay, so the widget tree always has the
        // same level. Otherwise, the state will be clear if the level is changed.
        let mut stack = widget::stack![keyboard];
        stack = stack.push_maybe(key_manager.popup_overlay(self.unit, self.size_p));
        stack.into()
    }

    pub fn on_event(&mut self, event: LayoutEvent) {
        match event {
            LayoutEvent::ToggleSetting => self.show_setting = !self.show_setting,
        }
    }
}

#[derive(Clone, Debug)]
pub enum LayoutEvent {
    ToggleSetting,
}

impl From<LayoutEvent> for Message {
    fn from(value: LayoutEvent) -> Self {
        Self::LayoutEvent(value)
    }
}
