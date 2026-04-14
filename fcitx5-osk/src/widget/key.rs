use std::collections::HashSet;

use iced::{
    advanced::{
        layout, overlay, renderer,
        widget::{tree, Operation, Tree},
        Clipboard, Layout, Shell, Widget,
    },
    border::Radius,
    event::Status,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    touch::{Event as TouchEvent, Finger as TouchFinger},
    Border, Color, Element, Event, Length, Padding, Rectangle, Size, Vector,
};

/// Local state of the [`Key`].
#[derive(Default)]
struct KeyState {
    fingers: HashSet<Option<TouchFinger>>,
    hovered: bool,
}

impl KeyState {
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

#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub pressed: bool,
    pub cancelled: bool,
    pub finger: Option<TouchFinger>,
    pub bounds: Rectangle,
}

pub trait AsThemeRef {
    fn as_ref(&self) -> &iced::Theme;
}

impl AsThemeRef for iced::Theme {
    fn as_ref(&self) -> &iced::Theme {
        self
    }
}

/// A widget works like MouseArea, Emit messages on mouse press/release events and finger press/lift/lost events.
pub struct Key<'a, Message, PressCb, ReleaseCb, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    width: Length,
    height: Length,
    padding: Padding,
    on_press_with: Option<PressCb>,
    on_release_with: Option<ReleaseCb>,
    border_radius: Radius,
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
    Key<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
{
    /// The callback for getting a message on a press event.
    pub fn on_press_with<NewPressCb>(
        self,
        cb: Option<NewPressCb>,
    ) -> Key<'a, Message, NewPressCb, ReleaseCb, Theme, Renderer> {
        let Key {
            content,
            width,
            height,
            padding,
            on_press_with: _on_press_with,
            on_release_with,
            border_radius,
        } = self;
        Key {
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
    ) -> Key<'a, Message, PressCb, NewReleaseCb, Theme, Renderer> {
        let Key {
            content,
            width,
            height,
            padding,
            on_press_with,
            on_release_with: _on_release_with,
            border_radius,
        } = self;
        Key {
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

pub type DummyCb<Message> = fn(KeyEvent) -> Message;

impl<'a, Message, Theme, Renderer>
    Key<'a, Message, DummyCb<Message>, DummyCb<Message>, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a [`Key`] with the given content.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Self {
            content,
            width: size.width.fluid(),
            height: size.height.fluid(),
            padding: Default::default(),
            border_radius: Default::default(),
            on_press_with: Default::default(),
            on_release_with: Default::default(),
        }
    }
}

impl<Message, PressCb, ReleaseCb, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Key<'_, Message, PressCb, ReleaseCb, Theme, Renderer>
where
    Message: Clone,
    PressCb: 'static + Fn(KeyEvent) -> Message,
    ReleaseCb: 'static + Fn(KeyEvent) -> Message,
    Theme: AsThemeRef,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<KeyState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(KeyState::default())
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

        if is_mouse_over && (self.on_press_with.is_some() || self.on_release_with.is_some()) {
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
        let state: &KeyState = tree.state.downcast_ref();
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
    From<Key<'a, Message, PressCb, ReleaseCb, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    PressCb: 'static + Fn(KeyEvent) -> Message,
    ReleaseCb: 'static + Fn(KeyEvent) -> Message,
    Theme: 'a + AsThemeRef,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        widget: Key<'a, Message, PressCb, ReleaseCb, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(widget)
    }
}

/// Processes the given [`Event`] and updates the [`KeyState`] of an [`Key`]
/// accordingly.
fn update<Message, PressCb, ReleaseCb, Theme, Renderer>(
    widget: &mut Key<'_, Message, PressCb, ReleaseCb, Theme, Renderer>,
    tree: &mut Tree,
    event: &Event,
    layout: Layout<'_>,
    cursor: MouseCursor,
    shell: &mut Shell<'_, Message>,
) where
    Message: Clone,
    PressCb: 'static + Fn(KeyEvent) -> Message,
    ReleaseCb: 'static + Fn(KeyEvent) -> Message,
{
    if widget.on_press_with.is_none() && widget.on_release_with.is_none() {
        return;
    }

    let state: &mut KeyState = tree.state.downcast_mut();

    let bounds = layout.bounds();
    if state.hovered != cursor.is_over(bounds) {
        state.hovered = !state.hovered;
        shell.request_redraw();
    }

    let (pressed, cancelled, finger, position) = match *event {
        Event::Mouse(MouseEvent::ButtonPressed(MouseButton::Left)) => {
            (true, false, None, cursor.position())
        }
        Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
            (false, false, None, cursor.position())
        }
        Event::Touch(TouchEvent::FingerPressed { id, position }) => {
            (true, false, Some(id), Some(position))
        }
        Event::Touch(TouchEvent::FingerLifted { id, position }) => {
            (false, false, Some(id), Some(position))
        }
        Event::Touch(TouchEvent::FingerLost { id, position }) => {
            (false, true, Some(id), Some(position))
        }
        _ => return,
    };

    if pressed {
        if let (false, Some(position)) = (state.is_pressed(&finger), position) {
            if bounds.contains(position) {
                tracing::trace!(
                    "key[{:?}] is pressed at {:?} by finger {:?}",
                    bounds,
                    position,
                    finger
                );
                if !state.has_finger_pressed() {
                    if let Some(cb) = &widget.on_press_with {
                        shell.publish(cb(KeyEvent {
                            pressed,
                            cancelled,
                            finger,
                            bounds,
                        }));
                    }
                }
                state.finger_pressed(finger);
                shell.capture_event();
            }
        }
    } else if state.is_pressed(&finger) {
        state.finger_released(&finger);
        tracing::trace!(
            "key[{:?}] is released by finger {:?}, pressed: {}",
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
                shell.publish(cb(KeyEvent {
                    pressed,
                    cancelled,
                    finger,
                    bounds,
                }));
                shell.capture_event();
            }
        }
    }
}

/// Local state of the [`PopupKey`].
#[derive(Default)]
struct PopupKeyState {
    is_active: bool,
}

/// A widget works like MouseArea, Emit messages on mouse enter/leave events and finger move event.
pub struct PopupKey<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    finger: Option<TouchFinger>,
    width: Length,
    height: Length,
    padding: Padding,
    on_enter: Option<Message>,
    on_exit: Option<Message>,
    border_radius: Radius,
}

impl<Message, Theme, Renderer> PopupKey<'_, Message, Theme, Renderer> {
    /// The message to emit on a enter event.
    #[must_use]
    pub fn on_enter(mut self, message: Message) -> Self {
        self.on_enter = Some(message);
        self
    }

    /// The message to emit on a exit event.
    #[must_use]
    pub fn on_exit(mut self, message: Message) -> Self {
        self.on_exit = Some(message);
        self
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

impl<'a, Message, Theme, Renderer> PopupKey<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a [`PopupKey`] with the given content.
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        finger: Option<TouchFinger>,
    ) -> Self {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Self {
            content,
            finger,
            width: size.width.fluid(),
            height: size.height.fluid(),
            padding: Default::default(),
            border_radius: Default::default(),
            on_enter: Default::default(),
            on_exit: Default::default(),
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for PopupKey<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: AsThemeRef,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<PopupKeyState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(PopupKeyState::default())
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

        if self.on_enter.is_none() && self.on_exit.is_none() {
            return;
        }

        let (finger, position) = match *event {
            Event::Mouse(MouseEvent::CursorMoved { position }) => (None, position),
            Event::Touch(TouchEvent::FingerMoved { id, position }) => (Some(id), position),
            _ => return,
        };

        if finger == self.finger {
            let state: &mut PopupKeyState = tree.state.downcast_mut();
            let is_hovered = layout.bounds().contains(position);
            match (is_hovered, state.is_active, &self.on_enter, &self.on_exit) {
                (true, false, Some(on_enter), _) => {
                    state.is_active = true;
                    shell.publish(on_enter.clone());
                    shell.capture_event();
                }
                (false, true, _, Some(on_exit)) => {
                    state.is_active = false;
                    shell.publish(on_exit.clone());
                    shell.capture_event();
                }
                _ => {}
            }
        }
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

        if is_mouse_over && (self.on_enter.is_some() || self.on_exit.is_some()) {
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
        let state: &PopupKeyState = tree.state.downcast_ref();
        let background = if state.is_active {
            theme.as_ref().extended_palette().primary.strong.color
        } else {
            // use the background of outside container, theme.as_ref().extended_palette().primary.weak.color
            Color::TRANSPARENT
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                // It should be the same as the outside container
                border: Border::default().rounded(self.border_radius),
                ..Default::default()
            },
            background,
        );
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

impl<'a, Message, Theme, Renderer> From<PopupKey<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + AsThemeRef,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        widget: PopupKey<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(widget)
    }
}
