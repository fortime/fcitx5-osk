use iced::{
    Event, Size,
    advanced::{Clipboard, Layout, Shell, layout, mouse::Cursor, overlay, renderer},
};
use iced_futures::core::widget::Operation;

use crate::misc::LocalShell as _;

pub trait Composer<Message, InnerMessage, Renderer> {
    #[allow(clippy::too_many_arguments)]
    fn compose(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        message: InnerMessage,
    );
}

/// An overlay composing messages from its inner overlay
pub struct ComposerOverlay<'a, Message, InnerMessage, Theme, Renderer> {
    composer: Box<dyn Composer<Message, InnerMessage, Renderer> + 'a>,
    content: overlay::Element<'a, InnerMessage, Theme, Renderer>,
}

impl<'a, Message, InnerMessage, Theme, Renderer>
    ComposerOverlay<'a, Message, InnerMessage, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Message: 'a,
    InnerMessage: 'a,
    Theme: 'a,
    Renderer: 'a,
{
    pub fn overlay<'b, C>(
        composer: C,
        content: overlay::Element<'b, InnerMessage, Theme, Renderer>,
    ) -> overlay::Element<'b, Message, Theme, Renderer>
    where
        'a: 'b,
        C: Composer<Message, InnerMessage, Renderer> + 'b,
    {
        overlay::Element::new(Box::new(ComposerOverlay {
            composer: Box::new(composer) as _,
            content,
        }))
    }
}

impl<'a, Message, InnerMessage, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer>
    for ComposerOverlay<'a, Message, InnerMessage, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        self.content.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
    ) {
        self.content
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let mut local_messages = vec![];
        let mut local_shell = shell.local(&mut local_messages);
        self.content.as_overlay_mut().update(
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local_shell,
        );
        drop(local_shell);

        for local_message in local_messages {
            self.composer.compose(
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                local_message,
            );
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.content
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        layout: Layout<'b>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if let Some(o) = self.content.as_overlay_mut().overlay(layout, renderer) {
            Some(ComposerOverlay::overlay(
                NestedComposer {
                    inner: &mut self.composer,
                },
                o,
            ))
        } else {
            None
        }
    }
}

struct NestedComposer<'a, 'b, Message, InnerMessage, Renderer> {
    inner: &'a mut Box<dyn Composer<Message, InnerMessage, Renderer> + 'b>,
}

impl<'a, 'b, Message, InnerMessage, Renderer> Composer<Message, InnerMessage, Renderer>
    for NestedComposer<'a, 'b, Message, InnerMessage, Renderer>
{
    fn compose(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        message: InnerMessage,
    ) {
        self.inner
            .compose(event, layout, cursor, renderer, clipboard, shell, message);
    }
}
