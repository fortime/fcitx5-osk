use std::{mem, rc::Rc};

use anyhow::Result;
use iced::{alignment::Horizontal, widget::{Column, Text}, Padding, Size};

use crate::layout::{KeyAreaLayout, KeyManager};

pub struct LayoutState {
    size_p: (u16, u16),
    unit: u16,
    padding: Padding,
    toolbar_layout: (),
    key_area_layout: Rc<KeyAreaLayout>,
}

impl LayoutState {
    const MIN_P: u16 = 640;

    const MIN_PADDING_P: u16 = 5;

    const TOOLBAR_HEIGHT: u16 = 6;

    pub fn new(width_p: u16, key_area_layout: Rc<KeyAreaLayout>) -> Result<Self> {
        let mut res = Self {
            size_p: (width_p, 0),
            unit: Default::default(),
            padding: Default::default(),
            toolbar_layout: Default::default(),
            key_area_layout,
        };
        res.calculate_size()?;
        Ok(res)
    }

    fn calculate_size(&mut self) -> Result<()> {
        let mut width_p = self.size_p.0;
        // when width or height mod 4 = 1, the size of this layout is not the same as the size of
        // window. So, when width or height mod 4 = 1, width or height will be increased by 1.
        if width_p % 4 == 1 {
            width_p += 1;
        }
        if width_p < Self::MIN_P {
            anyhow::bail!("width is too small: {}", width_p);
        }
        let trimmed_width_p = width_p - Self::MIN_PADDING_P * 2;
        let unit = self.key_area_layout.unit_within(trimmed_width_p);
        let key_area_size_p = self.key_area_layout.size_p(unit);

        self.unit = unit;
        let height_p_without_padding = key_area_size_p.1 + 4 + (Self::TOOLBAR_HEIGHT + 1) * unit;
        let mut height_p = height_p_without_padding + Self::MIN_PADDING_P * 2;
        if height_p % 4 == 1 {
            height_p += 1;
        }
        self.size_p = (width_p, height_p);
        self.padding = Padding::from([
            (height_p - height_p_without_padding) as f32 / 2.0,
            (width_p - key_area_size_p.0) as f32 / 2.0,
        ]);
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

    pub fn update_width(&mut self, mut width_p: u16) -> bool {
        mem::swap(&mut self.size_p.0, &mut width_p);
        if let Err(e) = self.calculate_size() {
            tracing::debug!("failed to update width: {e}, recovering.");
            // recover
            mem::swap(&mut self.size_p.0, &mut width_p);
            false
        } else {
            true
        }
    }

    pub(super) fn update_key_area_layout(&mut self, mut key_area_layout: Rc<KeyAreaLayout>) -> bool {
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

    pub fn to_element<'a, 'b, KM, M>(&'a self, input: &'b str, manager: &'b KM) -> Column<'b, M>
    where
        KM: KeyManager<Message = M>,
        M: 'static,
    {
        let size = self.size();
        Column::new()
            .align_x(Horizontal::Center)
            .width(size.width)
            .height(size.height)
            .padding(self.padding)
            .spacing(self.unit)
            .push(Text::new(input).height(Self::TOOLBAR_HEIGHT * self.unit))
            .push(self.key_area_layout.to_element(self.unit, manager))
    }
}
