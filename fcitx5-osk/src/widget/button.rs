use std::collections::HashSet;

use iced::{
    advanced::{
        layout, overlay, renderer,
        widget::{tree, Operation, Tree},
        Clipboard, Layout, Shell, Widget,
    },
    border::{self, Radius},
    event::Status,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    touch::{Event as TouchEvent, Finger as TouchFinger},
    widget::{button::DEFAULT_PADDING, container::Style as ContainerStyle, Container},
    Border, Element, Event, Length, Padding, Rectangle, Renderer, Size, Theme, Vector,
};

pub const BORDER_RADIUS: f32 = 5.;

/// Local state of the [`ExtButton`].
#[derive(Default)]
struct State {
    fingers: HashSet<Option<TouchFinger>>,
    hovered: bool,
}

impl State {
    fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn has_finger_pressed(&self) -> bool {
        !self.fingers.is_empty()
    }

    fn is_pressed(&self, finger: &Option<TouchFinger>) -> bool {
        self.fingers.contains(finger)
    }

    fn finger_pressed(&mut self, finger: Option<TouchFinger>) {
        self.fingers.insert(finger);
    }

    fn finger_released(&mut self, finger: &Option<TouchFinger>) {
        self.fingers.remove(finger);
    }
}

pub trait AsThemeRef {
    fn as_ref(&self) -> &iced::Theme;
}

impl AsThemeRef for iced::Theme {
    fn as_ref(&self) -> &iced::Theme {
        self
    }
}

/// An extend button widget, with on_press and on_release
pub struct ExtButton<
    'a,
    Message,
    PressCb,
    ReleaseCb,
    Theme = iced::Theme,
    Renderer = iced::Renderer,
> {
    content: Element<'a, Message, Theme, Renderer>,
    width: Length,
    height: Length,
    padding: Padding,
    on_press_with: Option<PressCb>,
    on_release_with: Option<ReleaseCb>,
    border_radius: Radius,
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
    ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
{
    /// The callback for getting a message on a press event.
    pub fn on_press_with<NewPressCb>(
        self,
        cb: Option<NewPressCb>,
    ) -> ExtButton<'a, Message, NewPressCb, ReleaseCb, Theme, Renderer> {
        let ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with: _on_press_with,
            on_release_with,
            border_radius,
        } = self;
        ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with: cb,
            on_release_with,
            border_radius,
        }
    }

    /// The callback for getting a message on a release event.
    pub fn on_release_with<NewReleaseCb>(
        self,
        cb: Option<NewReleaseCb>,
    ) -> ExtButton<'a, Message, PressCb, NewReleaseCb, Theme, Renderer> {
        let ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with,
            on_release_with: _on_release_with,
            border_radius,
        } = self;
        ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with,
            on_release_with: cb,
            border_radius,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn border_radius(mut self, border_radius: impl Into<Radius>) -> Self {
        self.border_radius = border_radius.into();
        self
    }
}

pub type DummyCb<Message> = fn() -> Message;

impl<'a, Message, Theme, Renderer>
    ExtButton<'a, Message, DummyCb<Message>, DummyCb<Message>, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a [`ExtButton`] with the given content.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Self {
            content,
            width: size.width.fluid(),
            height: size.height.fluid(),
            on_press_with: Default::default(),
            on_release_with: Default::default(),
            padding: Default::default(),
            border_radius: Default::default(),
        }
    }
}

impl<Message, PressCb, ReleaseCb, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ExtButton<'_, Message, PressCb, ReleaseCb, Theme, Renderer>
where
    Message: Clone,
    PressCb: 'static + Fn() -> Message,
    ReleaseCb: 'static + Fn() -> Message,
    Theme: AsThemeRef,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::padded(limits, self.width, self.height, self.padding, |limits| {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        })
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: MouseCursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.event_status() == Status::Captured {
            return;
        }

        update(self, tree, event, layout, cursor, shell);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: MouseCursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> MouseInteraction {
        let is_mouse_over = cursor.is_over(layout.bounds());

        if is_mouse_over && self.on_press_with.is_some() {
            MouseInteraction::Pointer
        } else {
            MouseInteraction::default()
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
        let state: &State = tree.state.downcast_ref();
        let background = if state.has_finger_pressed() {
            theme.as_ref().extended_palette().primary.strong.color
        } else if state.is_hovered() {
            theme.as_ref().extended_palette().primary.weak.color
        } else {
            theme.as_ref().extended_palette().background.base.color
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border::default().rounded(self.border_radius),
                ..Default::default()
            },
            background,
        );
        // after padding we should use layout.children[0] instead of layout to draw content
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout
                .children()
                .next()
                .expect("it should have content layout"),
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
    From<ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    PressCb: 'static + Fn() -> Message,
    ReleaseCb: 'static + Fn() -> Message,
    Theme: 'a + AsThemeRef,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        widget: ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(widget)
    }
}

/// Processes the given [`Event`] and updates the [`State`] of an [`ExtButton`]
/// accordingly.
fn update<Message, PressCb, ReleaseCb, Theme, Renderer>(
    widget: &mut ExtButton<'_, Message, PressCb, ReleaseCb, Theme, Renderer>,
    tree: &mut Tree,
    event: &Event,
    layout: Layout<'_>,
    cursor: MouseCursor,
    shell: &mut Shell<'_, Message>,
) where
    Message: Clone,
    PressCb: 'static + Fn() -> Message,
    ReleaseCb: 'static + Fn() -> Message,
{
    if widget.on_press_with.is_none() && widget.on_release_with.is_none() {
        return;
    }

    let state: &mut State = tree.state.downcast_mut();

    let bounds = layout.bounds();
    if state.hovered != cursor.is_over(bounds) {
        state.hovered = !state.hovered;
        shell.request_redraw();
    }

    let (pressed, finger, position) = match *event {
        Event::Mouse(MouseEvent::ButtonPressed(MouseButton::Left)) => {
            (true, None, cursor.position())
        }
        Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
            (false, None, cursor.position())
        }
        Event::Touch(TouchEvent::FingerPressed { id, position }) => {
            (true, Some(id), Some(position))
        }
        Event::Touch(TouchEvent::FingerLifted { id, position }) => {
            (false, Some(id), Some(position))
        }
        Event::Touch(TouchEvent::FingerLost { id, position }) => (false, Some(id), Some(position)),
        _ => return,
    };

    if pressed {
        if let (false, Some(position)) = (state.is_pressed(&finger), position) {
            if bounds.contains(position) {
                tracing::trace!(
                    "ExtButton[{:?}] is pressed at {:?} by finger {:?}",
                    bounds,
                    position,
                    finger
                );
                if !state.has_finger_pressed() {
                    if let Some(cb) = &widget.on_press_with {
                        shell.publish(cb());
                        shell.capture_event();
                    }
                }
                state.finger_pressed(finger);
            }
        }
    } else if state.is_pressed(&finger) {
        state.finger_released(&finger);
        tracing::trace!(
            "ExtButton[{:?}] is released by finger {:?}, pressed: {}",
            bounds,
            finger,
            state.fingers.len(),
        );
        if !state.has_finger_pressed() {
            // there is no finger is pressed, set hovered to false
            if finger.is_some() {
                state.hovered = false;
            }
            if let Some(cb) = widget.on_release_with.as_ref() {
                shell.publish(cb());
                shell.capture_event();
            }
        }
    }
}

pub fn button_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Container<'a, Message, Theme, Renderer> {
    Container::new(content)
        .center_y(Length::Shrink)
        .center_x(Length::Shrink)
        .style(|theme: &Theme| ContainerStyle {
            background: Some(theme.extended_palette().background.base.color.into()),
            border: border::rounded(BORDER_RADIUS),
            ..Default::default()
        })
        .padding(DEFAULT_PADDING)
}
