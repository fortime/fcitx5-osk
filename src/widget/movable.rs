use std::time::{Duration, Instant};

use iced::{
    event::Status,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    overlay,
    touch::{Event as TouchEvent, Finger as TouchFinger},
    Element, Event, Length, Point, Rectangle, Size, Vector,
};
use iced_futures::core::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};

/// Local state of the [`Movable`].
#[derive(Default)]
struct MovableState {
    prev_pointer: Option<(Option<TouchFinger>, Point)>,
    pointer: Option<(Option<TouchFinger>, Point)>,
    last: Option<Instant>,
}

/// A widget works like MouseArea, Emit messages on mouse enter/leave events and finger move event.
pub struct Movable<'a, Message, MoveCb, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    movable: bool,
    on_move_start: Option<Message>,
    on_move: MoveCb,
    on_move_end: Option<Message>,
}

impl<Message, MoveCb, Theme, Renderer> Movable<'_, Message, MoveCb, Theme, Renderer> {
    pub fn on_move_start(mut self, message: Message) -> Self {
        self.on_move_start = Some(message);
        self
    }

    pub fn on_move_end(mut self, message: Message) -> Self {
        self.on_move_end = Some(message);
        self
    }
}

impl<'a, Message, MoveCb, Theme, Renderer> Movable<'a, Message, MoveCb, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a [`Movable`] with the given content.
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        on_move: MoveCb,
        movable: bool,
    ) -> Self {
        let content = content.into();
        Self {
            content,
            on_move,
            movable,
            on_move_start: None,
            on_move_end: None,
        }
    }
}

impl<Message, MoveCb, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Movable<'_, Message, MoveCb, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
    MoveCb: Fn(Vector) -> Message,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MovableState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MovableState::default())
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
        let params = match event {
            Event::Mouse(MouseEvent::ButtonPressed(MouseButton::Left)) => {
                Some((true, false, None, cursor.position()))
            }
            Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
                Some((false, false, None, None))
            }
            Event::Touch(TouchEvent::FingerPressed { id, position }) => {
                Some((true, false, Some(id), Some(position)))
            }
            Event::Touch(TouchEvent::FingerLifted { id, .. })
            | Event::Touch(TouchEvent::FingerLost { id, .. }) => {
                Some((false, false, Some(id), None))
            }
            Event::Mouse(MouseEvent::CursorMoved { position }) => {
                Some((false, true, None, Some(position)))
            }
            Event::Touch(TouchEvent::FingerMoved { id, position }) => {
                Some((false, true, Some(id), Some(position)))
            }
            _ => None,
        };

        if !self.movable {
            if let Some((pressed, moved, cur_pointer, cur_position)) = params {
                let state: &mut MovableState = tree.state.downcast_mut();
                if pressed {
                    if let Some(cur_position) =
                        cur_position.filter(|p| layout.bounds().contains(*p))
                    {
                        state.prev_pointer = Some((cur_pointer, cur_position));
                    }
                } else if !moved {
                    state
                        .prev_pointer
                        .take_if(|(pointer, _)| *pointer == cur_pointer);
                }
            }
            if let Status::Captured = self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            ) {
                return Status::Captured;
            }
        } else {
            let Some((pressed, moved, cur_pointer, cur_position)) = params else {
                return Status::Ignored;
            };
            let state: &mut MovableState = tree.state.downcast_mut();
            if state.pointer.is_none() {
                state.pointer = state.prev_pointer.take();
            } else {
                state.prev_pointer.take();
            }
            if pressed {
                if state.pointer.is_some() {
                    return Status::Ignored;
                }
                if let Some(cur_position) = cur_position.filter(|p| layout.bounds().contains(*p)) {
                    state.pointer = Some((cur_pointer, cur_position));
                    if let Some(on_move_start) = self.on_move_start.clone() {
                        state.last = None;
                        shell.publish(on_move_start);
                    }
                    return Status::Captured;
                }
            } else if moved {
                if let Some(position) = state.pointer.as_ref().and_then(|(pointer, position)| {
                    if *pointer == cur_pointer {
                        Some(position)
                    } else {
                        None
                    }
                }) {
                    // update delta
                    if let Some(cur_position) = cur_position {
                        let now = Instant::now();
                        // avoid jitter
                        state
                            .last
                            .take_if(|last| now.duration_since(*last) > Duration::from_millis(50));
                        if state.last.is_none() {
                            shell.publish((self.on_move)(cur_position - *position));
                            state.last = Some(now);
                        }
                    }
                    return Status::Captured;
                }
            } else if let Some((_, _)) = state
                .pointer
                .take_if(|(pointer, _)| *pointer == cur_pointer)
            {
                // on_move_end
                if let Some(on_move_end) = self.on_move_end.clone() {
                    shell.publish(on_move_end);
                }
                return Status::Captured;
            }
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
        if self.movable {
            let state: &MovableState = tree.state.downcast_ref();
            if !cursor.is_over(layout.bounds()) {
                MouseInteraction::None
            } else if state.pointer.is_some() {
                MouseInteraction::Grabbing
            } else {
                MouseInteraction::Grab
            }
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
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

impl<'a, Message, MoveCb, Theme, Renderer> From<Movable<'a, Message, MoveCb, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
    MoveCb: 'a + Fn(Vector) -> Message,
{
    fn from(
        key: Movable<'a, Message, MoveCb, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(key)
    }
}
