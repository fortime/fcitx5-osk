use std::collections::HashSet;

use iced::{
    Background, Border, Color, Element, Event, Length, Padding, Rectangle, Renderer, Size, Theme,
    Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    border,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    touch::{Event as TouchEvent, Finger as TouchFinger},
    widget::{
        Container,
        button::{
            Catalog as ButtonCatalog, DEFAULT_PADDING, Status as ButtonStatus,
            Style as ButtonStyle, StyleFn as ButtonStyleFn,
        },
        container::Style as ContainerStyle,
    },
    window::Event as WindowEvent,
};

pub mod key;

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

pub trait ExtButtonCatalog: ButtonCatalog {
    fn custom_default<'a>() -> Self::Class<'a>;
}

impl ExtButtonCatalog for Theme {
    fn custom_default<'a>() -> Self::Class<'a> {
        Box::new(button_class)
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
> where
    Theme: ExtButtonCatalog,
{
    content: Element<'a, Message, Theme, Renderer>,
    width: Length,
    height: Length,
    padding: Padding,
    on_press_with: Option<PressCb>,
    on_release_with: Option<ReleaseCb>,
    class: Theme::Class<'a>,
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
    ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
where
    Theme: ExtButtonCatalog,
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
            class,
        } = self;
        ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with: cb,
            on_release_with,
            class,
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
            class,
        } = self;
        ExtButton {
            content,
            width,
            height,
            padding,
            on_press_with,
            on_release_with: cb,
            class,
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

    pub fn style(mut self, style: impl Fn(&Theme, ButtonStatus) -> ButtonStyle + 'a) -> Self
    where
        Theme::Class<'a>: From<ButtonStyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as ButtonStyleFn<'a, Theme>).into();
        self
    }

    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }
}

pub type DummyCb<Message> = fn() -> Message;

impl<'a, Message, Theme, Renderer>
    ExtButton<'a, Message, DummyCb<Message>, DummyCb<Message>, Theme, Renderer>
where
    Theme: ExtButtonCatalog,
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
            class: Theme::custom_default(),
        }
    }
}

impl<'a, Message, PressCb, ReleaseCb, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>
where
    Message: 'a + Clone,
    PressCb: 'a + Fn() -> Message,
    ReleaseCb: 'a + Fn() -> Message,
    Theme: 'a + ExtButtonCatalog,
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: MouseCursor,
        viewport: &Rectangle,
    ) {
        let state: &State = tree.state.downcast_ref();
        let status = if state.has_finger_pressed() {
            ButtonStatus::Pressed
        } else if state.is_hovered() {
            ButtonStatus::Hovered
        } else if self.on_press_with.is_some() || self.on_release_with.is_some() {
            ButtonStatus::Active
        } else {
            ButtonStatus::Disabled
        };
        let style = theme.style(&self.class, status);
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: style.border,
                shadow: style.shadow,
                snap: style.snap,
            },
            style
                .background
                .unwrap_or(Background::Color(Color::TRANSPARENT)),
        );
        // after padding we should use layout.children[0] instead of layout to draw content
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style {
                text_color: style.text_color,
            },
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
    PressCb: 'a + Fn() -> Message,
    ReleaseCb: 'a + Fn() -> Message,
    Theme: 'a + ExtButtonCatalog,
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
fn update<'a, Message, PressCb, ReleaseCb, Theme, Renderer>(
    widget: &mut ExtButton<'a, Message, PressCb, ReleaseCb, Theme, Renderer>,
    tree: &mut Tree,
    event: &Event,
    layout: Layout<'_>,
    cursor: MouseCursor,
    shell: &mut Shell<'_, Message>,
) where
    Message: 'a + Clone,
    PressCb: 'a + Fn() -> Message,
    ReleaseCb: 'a + Fn() -> Message,
    Theme: 'a + ExtButtonCatalog,
{
    let state: &mut State = tree.state.downcast_mut();

    if widget.on_press_with.is_none() && widget.on_release_with.is_none() {
        if state.hovered {
            state.hovered = false;
            shell.request_redraw();
        }
        return;
    }

    let bounds = layout.bounds();

    let (pressed, finger, position) = match *event {
        Event::Mouse(MouseEvent::ButtonPressed(MouseButton::Left)) => {
            (true, None, cursor.position())
        }
        Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
            (false, None, cursor.position())
        }
        Event::Touch(TouchEvent::FingerPressed { id, .. }) => {
            // NOTE the position won't be in the bounds if the button is inside a scrollable, use
            // the position from `cursor`
            (true, Some(id), cursor.position())
        }
        Event::Touch(TouchEvent::FingerLifted { id, .. }) => {
            // NOTE the position won't be in the bounds if the button is inside a scrollable, use
            // the position from `cursor`
            (false, Some(id), cursor.position())
        }
        Event::Touch(TouchEvent::FingerLost { id, .. }) => (false, Some(id), cursor.position()),
        Event::Mouse(MouseEvent::CursorMoved { .. }) => {
            // NOTE the position won't be in the bounds if the button is inside a scrollable, use
            // the position from `cursor`
            if !shell.is_event_captured() {
                if cursor.is_over(bounds) && !state.hovered {
                    state.hovered = true;
                    shell.request_redraw();
                } else if !cursor.is_over(bounds) && state.hovered {
                    state.hovered = false;
                    shell.request_redraw();
                }
            }
            return;
        }
        Event::Window(WindowEvent::RedrawRequested(_)) => {
            if state.hovered && !cursor.is_over(bounds) {
                state.hovered = false;
                shell.request_redraw();
            }
            return;
        }
        _ => return,
    };

    // Check after clearing stale hovered state
    if shell.is_event_captured() {
        return;
    }

    if pressed {
        if let (false, Some(position)) = (state.is_pressed(&finger), position)
            && bounds.contains(position)
        {
            tracing::trace!(
                "ExtButton[{:?}] is pressed at {:?} by finger {:?}",
                bounds,
                position,
                finger
            );
            if !state.has_finger_pressed() {
                if let Some(cb) = &widget.on_press_with {
                    shell.publish(cb());
                } else {
                    shell.request_redraw();
                }
            }
            shell.capture_event();
            state.finger_pressed(finger);
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
            if let Some(cb) = widget.on_release_with.as_ref() {
                shell.publish(cb());
            } else {
                shell.request_redraw();
            }
        }
        shell.capture_event();
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

pub fn center_y_button_container<'a, Message>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Container<'a, Message, Theme, Renderer> {
    button_container(content)
        .padding(DEFAULT_PADDING.vertical(0))
        .center_y(Length::Fill)
}

fn button_base_class(theme: &Theme) -> ButtonStyle {
    let pair = theme.extended_palette().background.base;
    ButtonStyle {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border::default().rounded(BORDER_RADIUS),
        ..Default::default()
    }
}

fn button_disabled(style: ButtonStyle) -> ButtonStyle {
    ButtonStyle {
        background: style
            .background
            .map(|background| background.scale_alpha(0.5)),
        text_color: style.text_color.scale_alpha(0.5),
        ..style
    }
}

pub fn button_class(theme: &Theme, status: ButtonStatus) -> ButtonStyle {
    let base = button_base_class(theme);
    let palette = theme.extended_palette();
    match status {
        ButtonStatus::Active => base,
        ButtonStatus::Hovered => ButtonStyle {
            background: Some(Background::Color(palette.primary.weak.color)),
            ..base
        },
        ButtonStatus::Pressed => ButtonStyle {
            background: Some(Background::Color(palette.primary.strong.color)),
            ..base
        },
        ButtonStatus::Disabled => button_disabled(base),
    }
}

pub fn button_danger_class(theme: &Theme, status: ButtonStatus) -> ButtonStyle {
    let base = button_base_class(theme);
    let palette = theme.extended_palette();
    match status {
        ButtonStatus::Active => base,
        ButtonStatus::Hovered => ButtonStyle {
            background: Some(Background::Color(palette.danger.weak.color)),
            ..base
        },
        ButtonStatus::Pressed => ButtonStyle {
            background: Some(Background::Color(palette.danger.strong.color)),
            ..base
        },
        ButtonStatus::Disabled => button_disabled(base),
    }
}

/// Highlight the text and make the background transparent
pub fn button_text_class(theme: &Theme, status: ButtonStatus) -> ButtonStyle {
    let mut base = button_base_class(theme);
    base.background = None;
    let palette = theme.extended_palette();
    match status {
        ButtonStatus::Active => base,
        ButtonStatus::Hovered => ButtonStyle {
            background: Some(Background::Color(palette.primary.weak.color)),
            ..base
        },
        ButtonStatus::Pressed => ButtonStyle {
            text_color: palette.primary.strong.color,
            ..base
        },
        ButtonStatus::Disabled => button_disabled(base),
    }
}

/// Highlight the text with danger class and make the background transparent
pub fn button_text_danger_class(theme: &Theme, status: ButtonStatus) -> ButtonStyle {
    let mut base = button_base_class(theme);
    base.background = None;
    let palette = theme.extended_palette();
    match status {
        ButtonStatus::Active => base,
        ButtonStatus::Hovered => ButtonStyle {
            background: Some(Background::Color(palette.danger.weak.color)),
            ..base
        },
        ButtonStatus::Pressed => ButtonStyle {
            text_color: palette.danger.strong.color,
            ..base
        },
        ButtonStatus::Disabled => button_disabled(base),
    }
}
