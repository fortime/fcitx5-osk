mod debugger;
mod key;
mod movable;
mod toggle;

mod scrollable {
    use iced::{
        border,
        widget::{
            container,
            scrollable::{AutoScroll, Rail, Scroller, Status, Style},
        },
        Color, Shadow, Theme, Vector,
    };

    /// Edited version of `iced::widget::scrollable::default`
    pub fn scrollable_style(theme: &Theme, status: Status) -> Style {
        let palette = theme.extended_palette();

        let scrollbar = Rail {
            background: Some(palette.background.weak.color.into()),
            border: border::rounded(2)
                .width(1)
                .color(palette.background.base.color),
            scroller: Scroller {
                background: palette.background.strong.color.into(),
                border: border::rounded(2)
                    .width(1)
                    .color(palette.background.base.color),
            },
        };

        // TODO not sure what this is
        let auto_scroll = AutoScroll {
            background: palette.background.base.color.scale_alpha(0.9).into(),
            border: border::rounded(u32::MAX)
                .width(1)
                .color(palette.background.base.text.scale_alpha(0.8)),
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.7),
                offset: Vector::ZERO,
                blur_radius: 2.0,
            },
            icon: palette.background.base.text.scale_alpha(0.8),
        };

        match status {
            Status::Active { .. } => Style {
                container: container::Style::default(),
                vertical_rail: scrollbar,
                horizontal_rail: scrollbar,
                gap: None,
                auto_scroll,
            },
            Status::Hovered {
                is_horizontal_scrollbar_hovered,
                is_vertical_scrollbar_hovered,
                ..
            } => {
                let hovered_scrollbar = Rail {
                    scroller: Scroller {
                        background: palette.primary.strong.color.into(),
                        ..scrollbar.scroller
                    },
                    ..scrollbar
                };

                Style {
                    container: container::Style::default(),
                    vertical_rail: if is_vertical_scrollbar_hovered {
                        hovered_scrollbar
                    } else {
                        scrollbar
                    },
                    horizontal_rail: if is_horizontal_scrollbar_hovered {
                        hovered_scrollbar
                    } else {
                        scrollbar
                    },
                    gap: None,
                    auto_scroll,
                }
            }
            Status::Dragged {
                is_horizontal_scrollbar_dragged,
                is_vertical_scrollbar_dragged,
                ..
            } => {
                let dragged_scrollbar = Rail {
                    scroller: Scroller {
                        background: palette.primary.base.color.into(),
                        ..scrollbar.scroller
                    },
                    ..scrollbar
                };

                Style {
                    container: container::Style::default(),
                    vertical_rail: if is_vertical_scrollbar_dragged {
                        dragged_scrollbar
                    } else {
                        scrollbar
                    },
                    horizontal_rail: if is_horizontal_scrollbar_dragged {
                        dragged_scrollbar
                    } else {
                        scrollbar
                    },
                    gap: None,
                    auto_scroll,
                }
            }
        }
    }
}

mod toggler {
    use iced::{
        widget::toggler::{self, Status, Style},
        Theme,
    };

    /// Edited version of `iced::widget::toggler::default`
    pub fn toggler_style(theme: &Theme, status: Status) -> Style {
        let mut style = toggler::default(theme, status);
        let palette = theme.extended_palette();
        style.background = palette.background.weak.color.into();
        style.background_border_color = palette.background.base.color;
        style.background_border_width = 1.;
        style.foreground = palette.background.base.color.into();
        style.foreground_border_color = palette.background.strong.color;
        style.foreground_border_width = 1.;
        match status {
            Status::Active { is_toggled } => {
                if is_toggled {
                    style.background = palette.primary.weak.color.into();
                    style.background_border_color = palette.primary.strong.color;
                }
            }
            Status::Hovered { is_toggled } => {
                if is_toggled {
                    style.background = palette.primary.weak.color.into();
                    style.background_border_color = palette.primary.strong.color;
                }
                style.foreground_border_color = palette.primary.strong.color;
                style.foreground_border_width = 1.;
            }
            Status::Disabled { .. } => {}
        }
        style
    }
}

#[allow(unused)]
pub use debugger::LayoutDebugger;
pub use key::{Key, KeyEvent, PopupKey};
pub use movable::Movable;
pub use scrollable::scrollable_style;
pub use toggle::{Toggle, ToggleCondition};
pub use toggler::toggler_style;
