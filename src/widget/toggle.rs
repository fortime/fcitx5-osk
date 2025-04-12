use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use iced::{
    event::Status,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    overlay,
    touch::{Event as TouchEvent, Finger as TouchFinger},
    Element, Event, Length, Rectangle, Size, Vector,
};
use iced_futures::core::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};

#[derive(Hash, PartialEq, Eq)]
enum Pointer {
    Finger(TouchFinger),
    Mouse(MouseButton),
}

/// Local state of the [`Toggle`].
#[derive(Default)]
struct ToggleState {
    condition: ToggleCondition,
    pointers: HashSet<Pointer>,
    last: Option<Instant>,
    toggled: bool,
}

impl ToggleState {
    fn update_condition(&mut self, condition: ToggleCondition) {
        if self.condition != condition {
            self.condition = condition;
            self.pointers.clear();
            self.last = None;
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ToggleCondition {
    LongPress(Duration),
    /// right click will be treated as double down
    #[default]
    DoubleDown,
    DoublePress(Duration),
}

/// A widget works like MouseArea, Emit messages on mouse enter/leave events and finger move event.
pub struct Toggle<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    condition: ToggleCondition,
    on_toggle: Option<Message>,
}

impl<Message, Theme, Renderer> Toggle<'_, Message, Theme, Renderer> {
    pub fn on_toggle(mut self, on_toggle: Message) -> Self {
        self.on_toggle = Some(on_toggle);
        self
    }
}

impl<'a, Message, Theme, Renderer> Toggle<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a [`Toggle`] with the given content.
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        condition: ToggleCondition,
    ) -> Self {
        let content = content.into();
        Self {
            content,
            condition,
            on_toggle: None,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Toggle<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ToggleState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ToggleState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: MouseCursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        let mut status = Status::Ignored;
        'out: {
            let Some(on_toggle) = &self.on_toggle else {
                break 'out;
            };

            let state: &mut ToggleState = tree.state.downcast_mut();

            let params = match event {
                Event::Mouse(MouseEvent::ButtonPressed(btn)) => {
                    Some((true, Pointer::Mouse(btn), cursor.position()))
                }
                Event::Mouse(MouseEvent::ButtonReleased(btn)) => {
                    Some((false, Pointer::Mouse(btn), cursor.position()))
                }
                Event::Touch(TouchEvent::FingerPressed { id, position }) => {
                    Some((true, Pointer::Finger(id), Some(position)))
                }
                Event::Touch(TouchEvent::FingerLifted { id, position })
                | Event::Touch(TouchEvent::FingerLost { id, position }) => {
                    Some((false, Pointer::Finger(id), Some(position)))
                }
                _ => None,
            };

            state.update_condition(self.condition);

            let now = Instant::now();
            let duration = match state.condition {
                ToggleCondition::LongPress(duration) | ToggleCondition::DoublePress(duration) => {
                    duration
                }
                ToggleCondition::DoubleDown => Duration::from_millis(10),
            };
            if state
                .last
                .take_if(|last| now.duration_since(*last) > duration)
                .is_some()
            {
                shell.publish(on_toggle.clone());
                state.toggled = true;
                break 'out;
            }

            let Some((pressed, pointer, position)) = params else {
                break 'out;
            };

            if pressed {
                if !position
                    .map(|p| layout.bounds().contains(p))
                    .unwrap_or(false)
                {
                    break 'out;
                }

                state.pointers.insert(pointer);
            } else if state.pointers.remove(&pointer) {
                state.toggled = !state.pointers.is_empty();
                if state.toggled {
                    // Don't sent release event to children.
                    status = Status::Captured;
                }
            }
            let expected_pointer_num = match state.condition {
                ToggleCondition::LongPress(_) => 1,
                ToggleCondition::DoubleDown => 2,
                ToggleCondition::DoublePress(_) => 2,
            };
            let pointer_num = if state.pointers.contains(&Pointer::Mouse(MouseButton::Right)) {
                state.pointers.len() + 1
            } else {
                state.pointers.len()
            };
            if pointer_num == expected_pointer_num && pressed {
                state.last = Some(now);
            } else if pointer_num != expected_pointer_num {
                state.last = None;
            }
        }

        if status == Status::Ignored {
            return self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event.clone(),
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        Status::Ignored
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: MouseCursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> MouseInteraction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: MouseCursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<Toggle<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(key: Toggle<'a, Message, Theme, Renderer>) -> Element<'a, Message, Theme, Renderer> {
        Element::new(key)
    }
}
