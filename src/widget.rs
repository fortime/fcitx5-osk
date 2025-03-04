use std::collections::HashSet;

use iced::{
    event::Status,
    mouse, overlay,
    touch::{self, Finger},
    Border, Element, Event, Length, Padding, Rectangle, Shadow, Size, Vector,
};
use iced_futures::core::{
    layout, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};

/// Local state of the [`Key`].
#[derive(Default)]
struct KeyState {
    fingers: HashSet<Option<Finger>>,
}

impl KeyState {
    fn has_finger_pressed(&self) -> bool {
        !self.fingers.is_empty()
    }

    fn is_pressed(&self, finger: &Option<Finger>) -> bool {
        self.fingers.contains(finger)
    }

    fn finger_pressed(&mut self, finger: Option<Finger>) {
        self.fingers.insert(finger);
    }

    fn finger_released(&mut self, finger: &Option<Finger>) {
        self.fingers.remove(finger);
    }
}

#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub is_pressed: bool,
    pub finger: Option<Finger>,
    pub bounds: Rectangle,
}

pub trait AsThemeRef {
    fn as_ref(&self) -> &iced::Theme;
}

impl AsThemeRef for iced::Theme {
    fn as_ref(&self) -> &iced::Theme {
        &self
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
        } = self;
        Key {
            content,
            width,
            height,
            padding,
            on_press_with: cb,
            on_release_with,
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
        } = self;
        Key {
            content,
            width,
            height,
            padding,
            on_press_with,
            on_release_with: cb,
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

    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
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
            on_press_with: None,
            on_release_with: None,
        }
    }
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Key<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
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
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::padded(limits, self.width, self.height, self.padding, |limits| {
            self.content
                .as_widget()
                .layout(&mut tree.children[0], renderer, limits)
        })
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
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        if let Status::Captured = self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event.clone(),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        ) {
            return Status::Captured;
        }

        update(self, tree, event, layout, cursor, shell)
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
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
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state: &KeyState = tree.state.downcast_ref();
        let background = if state.has_finger_pressed() {
            theme.as_ref().extended_palette().success.weak.color
        } else {
            theme.as_ref().extended_palette().success.strong.color
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border::default(),
                shadow: Shadow::default(),
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
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
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
        key: Key<'a, Message, PressCb, ReleaseCb, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(key)
    }
}

/// Processes the given [`Event`] and updates the [`KeyState`] of an [`Key`]
/// accordingly.
fn update<Message, PressCb, ReleaseCb, Theme, Renderer>(
    widget: &mut Key<'_, Message, PressCb, ReleaseCb, Theme, Renderer>,
    tree: &mut Tree,
    event: Event,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    shell: &mut Shell<'_, Message>,
) -> Status
where
    Message: Clone,
    PressCb: 'static + Fn(KeyEvent) -> Message,
    ReleaseCb: 'static + Fn(KeyEvent) -> Message,
{
    if widget.on_press_with.is_none() && widget.on_release_with.is_none() {
        return Status::Ignored;
    }

    let state: &mut KeyState = tree.state.downcast_mut();

    let (is_pressed, finger, position) = match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            (true, None, cursor.position())
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            (false, None, cursor.position())
        }
        Event::Touch(touch::Event::FingerPressed { id, position }) => {
            (true, Some(id), Some(position))
        }
        Event::Touch(touch::Event::FingerLifted { id, position })
        | Event::Touch(touch::Event::FingerLost { id, position }) => {
            (false, Some(id), Some(position))
        }
        _ => return Status::Ignored,
    };

    if is_pressed {
        match (
            state.is_pressed(&finger),
            widget.on_press_with.as_ref(),
            position,
        ) {
            (false, Some(cb), Some(position)) => {
                let bounds = layout.bounds();
                if bounds.contains(position) {
                    if !state.has_finger_pressed() {
                        shell.publish(cb(KeyEvent {
                            is_pressed,
                            finger,
                            bounds,
                        }));
                    }
                    state.finger_pressed(finger);
                    return Status::Captured;
                }
            }
            _ => {}
        }
    } else if state.is_pressed(&finger) {
        if let Some(cb) = widget.on_release_with.as_ref() {
            state.finger_released(&finger);
            if !state.has_finger_pressed() {
                shell.publish(cb(KeyEvent {
                    is_pressed,
                    finger,
                    bounds: layout.bounds(),
                }));
            }
            return Status::Captured;
        }
    }

    Status::Ignored
}

/// Local state of the [`PopupKey`].
#[derive(Default)]
struct PopupKeyState {
    is_active: bool,
}

/// A widget works like MouseArea, Emit messages on mouse enter/leave events and finger move event.
pub struct PopupKey<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    finger: Option<Finger>,
    width: Length,
    height: Length,
    padding: Padding,
    on_enter: Option<Message>,
    on_exit: Option<Message>,
}

impl<'a, Message, Theme, Renderer> PopupKey<'a, Message, Theme, Renderer> {
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

    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
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
        finger: Option<Finger>,
    ) -> Self {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Self {
            content,
            finger,
            width: size.width.fluid(),
            height: size.height.fluid(),
            padding: Default::default(),
            on_enter: None,
            on_exit: None,
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for PopupKey<'a, Message, Theme, Renderer>
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
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::padded(limits, self.width, self.height, self.padding, |limits| {
            self.content
                .as_widget()
                .layout(&mut tree.children[0], renderer, limits)
        })
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
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        if let Status::Captured = self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event.clone(),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        ) {
            return Status::Captured;
        }

        if self.on_enter.is_none() && self.on_exit.is_none() {
            return Status::Ignored;
        }

        let (finger, position) = match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => (None, position),
            Event::Touch(touch::Event::FingerMoved { id, position }) => (Some(id), position),
            _ => return Status::Ignored,
        };

        if finger == self.finger {
            let state: &mut PopupKeyState = tree.state.downcast_mut();
            let is_hovered = layout.bounds().contains(position);
            match (is_hovered, state.is_active, &self.on_enter, &self.on_exit) {
                (true, false, Some(on_enter), _) => {
                    state.is_active = true;
                    shell.publish(on_enter.clone());
                }
                (false, true, _, Some(on_exit)) => {
                    state.is_active = false;
                    shell.publish(on_exit.clone());
                }
                _ => {}
            }
        }

        Status::Ignored
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
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
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state: &PopupKeyState = tree.state.downcast_ref();
        let background = if state.is_active {
            theme.as_ref().extended_palette().danger.strong.color
        } else {
            theme.as_ref().extended_palette().danger.weak.color
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: Border::default(),
                shadow: Shadow::default(),
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
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<PopupKey<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + AsThemeRef,
    Renderer: 'a + renderer::Renderer,
{
    fn from(key: PopupKey<'a, Message, Theme, Renderer>) -> Element<'a, Message, Theme, Renderer> {
        Element::new(key)
    }
}
