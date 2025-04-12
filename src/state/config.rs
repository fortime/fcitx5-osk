use strum::IntoEnumIterator;

use crate::{
    app::Message,
    config::{Config, ConfigManager, IndicatorDisplay, Placement},
    layout::KeyboardManager,
};

use super::WindowManagerEvent;

macro_rules! on_update_event {
    ($event:ident, $config:expr, $(($variant:tt => $field:tt, $set:tt)),*  $(,)?) => {
        match $event {
            $(UpdateConfigEvent::$variant($field) => {
                if $config.$field().eq(&$field) {
                    false
                } else {
                    $config.$set($field);
                    true
                }
            },)*
        }
    }
}

pub struct StepDesc<T> {
    cur_value: fn(&dyn KeyboardManager) -> T,
    step: fn(&dyn KeyboardManager) -> T,
    on_increased: fn(&dyn KeyboardManager, T, T) -> Message,
    on_decreased: fn(&dyn KeyboardManager, T, T) -> Message,
}

impl<T> StepDesc<T> {
    pub fn cur_value(&self, km: &dyn KeyboardManager) -> T {
        (self.cur_value)(km)
    }

    pub fn on_increased(&self, km: &dyn KeyboardManager) -> Message {
        (self.on_increased)(km, self.cur_value(km), (self.step)(km))
    }

    pub fn on_decreased(&self, km: &dyn KeyboardManager) -> Message {
        (self.on_decreased)(km, self.cur_value(km), (self.step)(km))
    }
}

pub struct OwnedEnumDesc<T> {
    cur_value: fn(&dyn KeyboardManager) -> Option<T>,
    variants: Vec<T>,
    on_selected: fn(&dyn KeyboardManager, T) -> Message,
}

impl<T> OwnedEnumDesc<T> {
    pub fn cur_value(&self, km: &dyn KeyboardManager) -> Option<T> {
        (self.cur_value)(km)
    }

    pub fn variants(&self) -> &[T] {
        &self.variants
    }

    pub fn on_selected(&self, km: &dyn KeyboardManager, selected: T) -> Message {
        (self.on_selected)(km, selected)
    }
}

pub enum FieldType {
    StepU16(StepDesc<u16>),
    OwnedEnumPlacement(OwnedEnumDesc<Placement>),
    OwnedEnumIndicatorDisplay(OwnedEnumDesc<IndicatorDisplay>),
}

impl From<StepDesc<u16>> for FieldType {
    fn from(value: StepDesc<u16>) -> Self {
        Self::StepU16(value)
    }
}

impl From<OwnedEnumDesc<Placement>> for FieldType {
    fn from(value: OwnedEnumDesc<Placement>) -> Self {
        Self::OwnedEnumPlacement(value)
    }
}

impl From<OwnedEnumDesc<IndicatorDisplay>> for FieldType {
    fn from(value: OwnedEnumDesc<IndicatorDisplay>) -> Self {
        Self::OwnedEnumIndicatorDisplay(value)
    }
}

pub struct Field {
    name: &'static str,
    id: &'static str,
    typ: FieldType,
}

impl Field {
    pub fn name(&self) -> &str {
        self.name
    }

    #[allow(unused)]
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn typ(&self) -> &FieldType {
        &self.typ
    }
}

pub struct ConfigState {
    config_manager: ConfigManager,
    updatable_fields: Vec<Field>,
}

impl ConfigState {
    pub fn new(config_manager: ConfigManager) -> Self {
        Self {
            config_manager,
            updatable_fields: vec![
                Field {
                    name: "Size(unit)",
                    id: "size",
                    typ: StepDesc::<u16> {
                        cur_value: |km: &dyn KeyboardManager| km.unit(),
                        step: |km: &dyn KeyboardManager| {
                            let scale_factor = km.scale_factor();
                            let mut step = 1;
                            loop {
                                if (scale_factor * step as f32).fract() == 0.0 {
                                    break;
                                }
                                step += 1;
                            }
                            step
                        },
                        on_increased: |_, cur_value, delta| {
                            Message::from(WindowManagerEvent::UpdateUnit(cur_value + delta))
                        },
                        on_decreased: |_, cur_value, delta| {
                            if cur_value > delta {
                                Message::from(WindowManagerEvent::UpdateUnit(cur_value - delta))
                            } else {
                                Message::Nothing
                            }
                        },
                    }
                    .into(),
                },
                Field {
                    name: "Placement",
                    id: "placement",
                    typ: OwnedEnumDesc::<Placement> {
                        cur_value: |km: &dyn KeyboardManager| Some(km.config().placement()),
                        variants: Placement::iter().collect(),
                        on_selected: |_, p| Message::from(WindowManagerEvent::UpdatePlacement(p)),
                    }
                    .into(),
                },
                Field {
                    name: "Indicator Display",
                    id: "indicator_display",
                    typ: OwnedEnumDesc::<IndicatorDisplay> {
                        cur_value: |km: &dyn KeyboardManager| Some(km.config().indicator_display()),
                        variants: IndicatorDisplay::iter().collect(),
                        on_selected: |_, d| Message::from(WindowManagerEvent::UpdateIndicatorDisplay(d)),
                    }
                    .into(),
                },
            ],
        }
    }

    pub fn config(&self) -> &Config {
        self.config_manager.as_ref()
    }

    pub fn updatable_fields(&self) -> &[Field] {
        &self.updatable_fields
    }

    pub fn refresh(&mut self) {
        // clear temp values if needed
    }

    pub fn on_update_event(&mut self, event: UpdateConfigEvent) -> bool {
        let config = self.config_manager.as_mut();
        let updated = on_update_event!(
            event,
            config,
            (LandscapeWidth => landscape_width, set_landscape_width),
            (PortraitWidth => portrait_width, set_portrait_width),
            (Placement => placement, set_placement),
            (IndicatorDisplay => indicator_display, set_indicator_display),
            (Theme => theme, set_theme)
        );
        if updated {
            self.config_manager.try_write();
        }
        updated
    }
}

#[derive(Clone, Debug)]
pub enum UpdateConfigEvent {
    LandscapeWidth(u16),
    PortraitWidth(u16),
    Placement(Placement),
    IndicatorDisplay(IndicatorDisplay),
    Theme(String),
}

impl From<UpdateConfigEvent> for Message {
    fn from(value: UpdateConfigEvent) -> Self {
        Self::UpdateConfigEvent(value)
    }
}
