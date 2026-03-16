use std::cell::RefCell;

use iced::{
    advanced::{
        layout, overlay, renderer,
        widget::{tree, Operation, Tree},
        Clipboard, Layout, Shell, Widget,
    },
    event::Status,
    mouse::{Cursor, Interaction},
    Element, Event, Length, Rectangle, Size, Vector,
};

pub struct LayoutDebugger<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    name: String,
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> LayoutDebugger<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new(name: &str, content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
        }
    }
}

#[derive(Default)]
struct LayoutDebuggerState {
    bounds: Rectangle,
    viewport: RefCell<Rectangle>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for LayoutDebugger<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LayoutDebuggerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(LayoutDebuggerState::default())
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
        let l = self
            .content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits);
        let state: &mut LayoutDebuggerState = tree.state.downcast_mut();
        let bounds = l.bounds();
        if bounds != state.bounds {
            tracing::debug!(
                "The layout of {} is changed from {:?} to {:?}",
                self.name,
                state.bounds,
                bounds
            );
            state.bounds = bounds;
        }
        l
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
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event.clone(),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> Interaction {
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
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        let state: &LayoutDebuggerState = tree.state.downcast_ref();
        let mut state_viewport = state.viewport.borrow_mut();
        if *viewport != *state_viewport {
            tracing::debug!(
                "The viewport of drawing {} is changed from {:?} to {:?}",
                self.name,
                state_viewport,
                viewport
            );
            *state_viewport = *viewport;
        }
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

impl<'a, Message, Theme, Renderer> From<LayoutDebugger<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        widget: LayoutDebugger<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(widget)
    }
}
