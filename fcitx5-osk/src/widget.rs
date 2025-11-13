mod key;
mod movable;
mod toggle;

mod scrollable {
    use iced::{
        border,
        widget::{
            container,
            scrollable::{Rail, Scroller, Status, Style},
        },
        Theme,
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
                color: palette.background.strong.color,
                border: border::rounded(2)
                    .width(1)
                    .color(palette.background.base.color),
            },
        };

        match status {
            Status::Active => Style {
                container: container::Style::default(),
                vertical_rail: scrollbar,
                horizontal_rail: scrollbar,
                gap: None,
            },
            Status::Hovered {
                is_horizontal_scrollbar_hovered,
                is_vertical_scrollbar_hovered,
            } => {
                let hovered_scrollbar = Rail {
                    scroller: Scroller {
                        color: palette.primary.strong.color,
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
                }
            }
            Status::Dragged {
                is_horizontal_scrollbar_dragged,
                is_vertical_scrollbar_dragged,
            } => {
                let dragged_scrollbar = Rail {
                    scroller: Scroller {
                        color: palette.primary.base.color,
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
                }
            }
        }
    }
}

pub use key::{Key, KeyEvent, PopupKey};
pub use movable::Movable;
pub use scrollable::scrollable_style;
pub use toggle::{Toggle, ToggleCondition};
