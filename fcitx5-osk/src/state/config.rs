use std::{
    collections::HashMap,
    fmt::{Display, Formatter, Result as FmtResult},
};

use iced::{Task, Theme};
use strum::IntoEnumIterator;

use crate::{
    app::Message,
    config::{Config, ConfigManager, IndicatorDisplay, Placement},
    state::{ImEvent, StateExtractor, ThemeEvent, WindowManagerEvent},
    window::WindowManagerMode,
};

macro_rules! on_update_event {
    ($event:ident, $config:expr, $(@$variant:ident => {$eq:expr, $set:ident $(, $message_cb:expr)?}),*$(,)? $($pat: pat => $raw_expr: expr),* $(,)?) => {
        match $event {
            $(UpdateConfigEvent::$variant(v) => {
                if $eq($config, &v) {
                    (false, None)
                } else {
                    $config.$set(v.clone());
                    #[allow(unused_mut, unused_assignments)]
                    let mut message = None;
                    $( message = Some($message_cb(v)); )?
                    (true, message)
                }
            },)*
            $($pat => $raw_expr,)*
        }
    }
}

macro_rules! config_eq {
    ($field:tt) => {
        |c: &Config, v| c.$field().eq(v)
    };
}

macro_rules! option_config_eq {
    ($field:tt) => {
        |c: &Config, v| c.$field().filter(|f| (*f).eq(v)).is_some()
    };
}

pub struct ValueAndDescription<T> {
    value: T,
    desc: String,
}

impl<T> Display for ValueAndDescription<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&self.desc)
    }
}

impl<T> Clone for ValueAndDescription<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            desc: self.desc.clone(),
        }
    }
}

impl<T> PartialEq for ValueAndDescription<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

pub struct StepDesc<T> {
    cur_value: fn(&dyn StateExtractor) -> T,
    step: fn(&dyn StateExtractor) -> T,
    on_increased: fn(&dyn StateExtractor, T, T) -> Option<Message>,
    on_decreased: fn(&dyn StateExtractor, T, T) -> Option<Message>,
}

impl<T> StepDesc<T> {
    pub fn cur_value(&self, state: &dyn StateExtractor) -> T {
        (self.cur_value)(state)
    }

    pub fn on_increased(&self, state: &dyn StateExtractor) -> Option<Message> {
        (self.on_increased)(state, self.cur_value(state), (self.step)(state))
    }

    pub fn on_decreased(&self, state: &dyn StateExtractor) -> Option<Message> {
        (self.on_decreased)(state, self.cur_value(state), (self.step)(state))
    }
}

pub struct OwnedEnumDesc<T> {
    cur_value: fn(&dyn StateExtractor) -> Option<T>,
    variants: Vec<T>,
    is_enabled: fn(&dyn StateExtractor) -> bool,
    on_selected: fn(&dyn StateExtractor, T) -> Message,
}

impl<T> OwnedEnumDesc<T> {
    pub fn cur_value(&self, state: &dyn StateExtractor) -> Option<T> {
        (self.cur_value)(state)
    }

    pub fn variants(&self) -> &[T] {
        &self.variants
    }

    pub fn is_enabled(&self, state: &dyn StateExtractor) -> bool {
        (self.is_enabled)(state)
    }

    pub fn on_selected(&self, state: &dyn StateExtractor, selected: T) -> Message {
        (self.on_selected)(state, selected)
    }
}

pub struct EnumDesc<T> {
    cur_value: fn(&dyn StateExtractor) -> Option<&T>,
    variants: Vec<T>,
    is_enabled: fn(&dyn StateExtractor) -> bool,
    on_selected: fn(&dyn StateExtractor, T) -> Message,
}

impl<T> EnumDesc<T> {
    pub fn cur_value<'a>(&self, state: &'a dyn StateExtractor) -> Option<&'a T> {
        (self.cur_value)(state)
    }

    pub fn variants(&self) -> &[T] {
        &self.variants
    }

    pub fn is_enabled(&self, state: &dyn StateExtractor) -> bool {
        (self.is_enabled)(state)
    }

    pub fn on_selected(&self, state: &dyn StateExtractor, selected: T) -> Message {
        (self.on_selected)(state, selected)
    }
}

pub struct DynamicEnumDesc<T> {
    #[allow(clippy::type_complexity)]
    variants_and_selected:
        fn(&dyn StateExtractor) -> (Vec<ValueAndDescription<T>>, Option<ValueAndDescription<T>>),
    is_enabled: fn(&dyn StateExtractor) -> bool,
    on_selected: fn(&dyn StateExtractor, ValueAndDescription<T>) -> Message,
}

impl<T> DynamicEnumDesc<T> {
    pub fn variants_and_selected(
        &self,
        state: &dyn StateExtractor,
    ) -> (Vec<ValueAndDescription<T>>, Option<ValueAndDescription<T>>) {
        (self.variants_and_selected)(state)
    }

    pub fn is_enabled(&self, state: &dyn StateExtractor) -> bool {
        (self.is_enabled)(state)
    }

    pub fn on_selected(
        &self,
        state: &dyn StateExtractor,
        selected: ValueAndDescription<T>,
    ) -> Message {
        (self.on_selected)(state, selected)
    }
}

/// TODO Actually, this doesn't work because the keyboard ui doesn't accept input, you can't type text. How funny I am!
pub struct TextDesc {
    placeholder: fn(&Field, &dyn StateExtractor) -> Option<String>,
    init_value: fn(&dyn StateExtractor) -> Option<String>,
    is_enabled: fn(&dyn StateExtractor) -> bool,
    submit_message: fn(String) -> UpdateConfigEvent,
}

impl TextDesc {
    pub fn placeholder(&self, field: &Field, state: &dyn StateExtractor) -> Option<String> {
        (self.placeholder)(field, state)
    }

    pub fn cur_value(&self, field: &Field, state: &dyn StateExtractor) -> Option<String> {
        let init_value = (self.init_value)(state);
        if let Some((last_init_value, value)) = state.config_temp_text(field.id()) {
            if last_init_value.is_empty() && init_value.is_none() {
                return Some(value.to_string());
            }
            if init_value
                .as_ref()
                .filter(|v| *v == last_init_value)
                .is_some()
            {
                return Some(value.to_string());
            }
        }
        init_value
    }

    pub fn on_input_maybe(
        &self,
        field: &Field,
        state: &dyn StateExtractor,
    ) -> Option<Box<dyn Fn(String) -> Message>> {
        if !(self.is_enabled)(state) {
            return None;
        }

        let init_value = (self.init_value)(state).unwrap_or_default();
        let key = field.id().to_string();

        let handle = move |value| {
            Message::from(UpdateConfigEvent::ChangeTempText {
                key: key.clone(),
                init_value: init_value.clone(),
                value,
            })
        };
        Some(Box::new(handle))
    }

    pub fn on_paste_maybe(
        &self,
        field: &Field,
        state: &dyn StateExtractor,
    ) -> Option<Box<dyn Fn(String) -> Message>> {
        self.on_input_maybe(field, state)
    }

    pub fn on_submit_maybe(&self, field: &Field, state: &dyn StateExtractor) -> Option<Message> {
        if !(self.is_enabled)(state) {
            return None;
        }

        let init_value = (self.init_value)(state).unwrap_or_default();
        let key = field.id().to_string();

        Some(Message::from(UpdateConfigEvent::SubmitTempText {
            key,
            init_value,
            producer: self.submit_message,
        }))
    }
}

pub enum FieldType {
    StepU16(StepDesc<u16>),
    OwnedEnumPlacement(OwnedEnumDesc<Placement>),
    OwnedEnumIndicatorDisplay(OwnedEnumDesc<IndicatorDisplay>),
    EnumString(EnumDesc<String>),
    DynamicEnumString(DynamicEnumDesc<String>),
    Text(TextDesc),
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

impl From<EnumDesc<String>> for FieldType {
    fn from(value: EnumDesc<String>) -> Self {
        Self::EnumString(value)
    }
}

impl From<DynamicEnumDesc<String>> for FieldType {
    fn from(value: DynamicEnumDesc<String>) -> Self {
        Self::DynamicEnumString(value)
    }
}

impl From<TextDesc> for FieldType {
    fn from(value: TextDesc) -> Self {
        Self::Text(value)
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
    temp_texts: HashMap<String, (String, String)>,
}

impl ConfigState {
    pub fn new(config_manager: ConfigManager) -> Self {
        let themes = Theme::ALL
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Self {
            config_manager,
            updatable_fields: vec![
                Field {
                    name: "Size(unit)",
                    id: "size",
                    typ: StepDesc::<u16> {
                        cur_value: |state| state.unit(),
                        step: |state| {
                            let scale_factor = state.scale_factor();
                            let mut step = 1;
                            loop {
                                if (scale_factor * step as f32).fract() == 0.0 {
                                    break;
                                }
                                step += 1;
                            }
                            step
                        },
                        on_increased: |state, cur_value, delta| {
                            if state.window_manager_mode() == WindowManagerMode::Normal {
                                Some(Message::from(WindowManagerEvent::UpdateUnit(
                                    cur_value + delta,
                                )))
                            } else {
                                None
                            }
                        },
                        on_decreased: |state, cur_value, delta| {
                            if cur_value > delta
                                && state.window_manager_mode() == WindowManagerMode::Normal
                            {
                                Some(Message::from(WindowManagerEvent::UpdateUnit(
                                    cur_value - delta,
                                )))
                            } else {
                                None
                            }
                        },
                    }
                    .into(),
                },
                Field {
                    name: "Placement",
                    id: "placement",
                    typ: OwnedEnumDesc::<Placement> {
                        cur_value: |state| Some(state.placement()),
                        variants: Placement::iter().collect(),
                        is_enabled: |state| {
                            state.window_manager_mode() == WindowManagerMode::Normal
                        },
                        on_selected: |_, p| Message::from(WindowManagerEvent::UpdatePlacement(p)),
                    }
                    .into(),
                },
                Field {
                    name: "Indicator Display",
                    id: "indicator_display",
                    typ: OwnedEnumDesc::<IndicatorDisplay> {
                        cur_value: |state| Some(state.indicator_display()),
                        variants: IndicatorDisplay::iter().collect(),
                        is_enabled: |state| {
                            state.window_manager_mode() == WindowManagerMode::Normal
                        },
                        on_selected: |_, d| {
                            Message::from(WindowManagerEvent::UpdateIndicatorDisplay(d))
                        },
                    }
                    .into(),
                },
                Field {
                    name: "Dark Theme",
                    id: "dark_theme",
                    typ: EnumDesc::<String> {
                        cur_value: |state| state.config().dark_theme(),
                        variants: themes.clone(),
                        is_enabled: |_| true,
                        on_selected: |_, d| Message::from(UpdateConfigEvent::DarkTheme(d)),
                    }
                    .into(),
                },
                Field {
                    name: "Light Theme",
                    id: "light_theme",
                    typ: EnumDesc::<String> {
                        cur_value: |state| state.config().light_theme(),
                        variants: themes,
                        is_enabled: |_| true,
                        on_selected: |_, d| Message::from(UpdateConfigEvent::LightTheme(d)),
                    }
                    .into(),
                },
                preferred_output_name_field(),
                preferred_output_name_custom_field(),
            ],
            temp_texts: Default::default(),
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

    pub fn temp_text(&self, key: &str) -> Option<(&str, &str)> {
        self.temp_texts
            .get(key)
            .map(|(init_value, value)| (init_value.as_str(), value.as_str()))
    }

    pub fn on_update_event(&mut self, event: UpdateConfigEvent) -> Task<Message> {
        let config = self.config_manager.as_mut();
        let (updated, message) = on_update_event!(
            event,
            config,
            @LandscapeWidth => {
                config_eq!(landscape_width),
                set_landscape_width,
                |_| Message::from(ImEvent::ResetCandidateCursor)
            },
            @PortraitWidth => {
                config_eq!(portrait_width),
                set_portrait_width,
                |_| Message::from(ImEvent::ResetCandidateCursor)
            },
            @Placement => {config_eq!(placement), set_placement},
            @IndicatorDisplay => {config_eq!(indicator_display), set_indicator_display},
            @Theme => {
                config_eq!(theme),
                set_theme,
                |_| Message::from(ThemeEvent::Updated)
            },
            @DarkTheme => {option_config_eq!(dark_theme), set_dark_theme},
            @LightTheme => {option_config_eq!(light_theme), set_light_theme},
            @PreferredOutputName => {
                option_config_eq!(preferred_output_name),
                set_preferred_output_name,
                |v| Message::from(WindowManagerEvent::UpdatePreferredOutputName(v))
            },
            UpdateConfigEvent::ChangeTempText {key, init_value, value} => {
                tracing::error!("Update temp_text[{key}] to {value}, init value[{init_value}]");
                self.temp_texts.insert(key, (init_value, value));
                (false, None)
            },
            UpdateConfigEvent::SubmitTempText {key, init_value, producer} => {
                let mut message = None;
                if let Some((last_init_value, value)) = self.temp_texts.remove(&key) {
                    // update only if init_value is the same
                    if last_init_value == init_value && value != init_value {
                        message = Some(Message::from(producer(value)))
                    }
                };
                (false, message)
            },
        );
        if updated {
            self.config_manager.try_write();
        }
        message.map(Task::done).unwrap_or_else(Message::nothing)
    }
}

fn preferred_output_name_field() -> Field {
    Field {
        name: "Preferred Output",
        id: "preferred_output_name",
        typ: DynamicEnumDesc::<String> {
            variants_and_selected: |state| {
                let outputs = state.outputs();
                let mut variants = Vec::with_capacity(outputs.len());
                let mut selected = None;
                let preferred_output_name = state.config().preferred_output_name();

                for (name, description) in outputs {
                    if preferred_output_name.filter(|n| **n == name).is_some() {
                        selected = Some(output_name(name.clone(), description.clone()));
                    }
                    variants.push(output_name(name, description));
                }
                if selected.is_none() {
                    if let Some(name) = preferred_output_name {
                        selected = Some(ValueAndDescription {
                            value: name.clone(),
                            desc: format!("{name} (Not Connected)"),
                        })
                    }
                }
                (variants, selected)
            },
            is_enabled: |_| true,
            on_selected: |_, d| Message::from(UpdateConfigEvent::PreferredOutputName(d.value)),
        }
        .into(),
    }
}

fn preferred_output_name_custom_field() -> Field {
    Field {
        name: "Preferred Output(Custom)",
        id: "preferred_output_name",
        typ: TextDesc {
            placeholder: |_, _| Some("The name of the preferred output, like: DP-1".to_string()),
            is_enabled: |_| true,
            init_value: |state| state.config().preferred_output_name().cloned(),
            submit_message: |s| UpdateConfigEvent::PreferredOutputName(s),
        }
        .into(),
    }
}

fn output_name(name: String, description: String) -> ValueAndDescription<String> {
    if description.is_empty() {
        ValueAndDescription {
            value: name.clone(),
            desc: name,
        }
    } else {
        ValueAndDescription {
            value: name.clone(),
            desc: format!("{name} ({description})"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum UpdateConfigEvent {
    LandscapeWidth(u16),
    PortraitWidth(u16),
    Placement(Placement),
    IndicatorDisplay(IndicatorDisplay),
    Theme(String),
    DarkTheme(String),
    LightTheme(String),
    PreferredOutputName(String),
    ChangeTempText {
        key: String,
        init_value: String,
        value: String,
    },
    SubmitTempText {
        key: String,
        init_value: String,
        producer: fn(String) -> UpdateConfigEvent,
    },
}

impl From<UpdateConfigEvent> for Message {
    fn from(value: UpdateConfigEvent) -> Self {
        Self::UpdateConfigEvent(value)
    }
}
