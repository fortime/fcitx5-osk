#![allow(clippy::type_complexity)]

use std::{
    collections::HashSet,
    fmt::{Debug, Formatter, Result as FmtResult},
    marker::PhantomData,
    rc::Rc,
    slice,
    time::{Duration, Instant},
};

use iced::{
    Alignment, Element, Event, Length, Padding, Pixels, Point, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout,
        mouse::{Cursor, Interaction},
        overlay::{self, Group},
        renderer,
        text::Renderer as TextRenderer,
        widget::{Operation, Tree, operation::scrollable, tree},
    },
    alignment::Vertical,
    border,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    touch::{Event as TouchEvent, Finger as TouchFinger},
    widget::{
        Container, Id as WidgetId, PickList, Row, Scrollable, Space, Text, TextInput,
        button::{
            Catalog as ButtonCatalog, Status as ButtonStatus, Style as ButtonStyle,
            StyleFn as ButtonStyleFn,
        },
        container::{
            Catalog as ContainerCatalog, Style as ContainerStyle, StyleFn as ContainerStyleFn,
        },
        operation::AbsoluteOffset,
        pick_list::{
            self, Catalog as PickListCatalog, Status as PickListStatus, Style as PickListStyle,
            StyleFn as PickListStyleFn,
        },
        scrollable::Catalog as ScrollableCatalog,
        text::Catalog as TextCatalog,
        text_input::{
            self, Catalog as TextInputCatalog, Status as TextInputStatus, Style as TextInputStyle,
            StyleFn as TextInputStyleFn,
        },
    },
    window::Event as WindowEvent,
};
use iced_drop::widget::droppable::Droppable;

use crate::{
    misc::LocalShell,
    widget::{
        ExtButton, ExtButtonCatalog,
        button::{self},
        overlay::{Composer, ComposerOverlay},
    },
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
        tree.diff_children(slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        let params = match *event {
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
        } else {
            let Some((pressed, moved, cur_pointer, cur_position)) = params else {
                return;
            };
            let state: &mut MovableState = tree.state.downcast_mut();
            if state.pointer.is_none() {
                state.pointer = state.prev_pointer.take();
            } else {
                state.prev_pointer.take();
            }
            if pressed {
                if state.pointer.is_some() {
                    return;
                }
                if let Some(cur_position) = cur_position.filter(|p| layout.bounds().contains(*p)) {
                    state.pointer = Some((cur_pointer, cur_position));
                    if let Some(on_move_start) = self.on_move_start.clone() {
                        state.last = None;
                        shell.publish(on_move_start);
                    }
                    shell.capture_event();
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
                    shell.capture_event();
                }
            } else if let Some((_, _)) = state
                .pointer
                .take_if(|(pointer, _)| *pointer == cur_pointer)
            {
                // on_move_end
                if let Some(on_move_end) = self.on_move_end.clone() {
                    shell.publish(on_move_end);
                }
                shell.capture_event();
            }
        }
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

impl<'a, Message, MoveCb, Theme, Renderer> From<Movable<'a, Message, MoveCb, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
    MoveCb: 'a + Fn(Vector) -> Message,
{
    fn from(
        widget: Movable<'a, Message, MoveCb, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(widget)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MovableListEmptySlotLayout {
    before: u8,
    middle: Option<Rectangle>,
    after: u8,
}

impl MovableListEmptySlotLayout {
    fn new() -> Self {
        Self {
            before: 1,
            middle: None,
            after: 1,
        }
    }

    fn remove_padding(&mut self) {
        self.before = 0;
        self.after = 0;
    }
}

/// Local state of the [`MovableList`].
struct MovableListState {
    dragged: Option<(usize, Rectangle)>,
    dropping: Option<usize>,
}

impl MovableListState {
    fn is_dragged(&self, idx: usize) -> bool {
        self.dragged
            .filter(|(dragged_idx, _)| *dragged_idx == idx)
            .is_some()
    }
}

/// The theme catalog of a [`MovableList`].
pub trait MovableListCatalog: ContainerCatalog + ExtButtonCatalog + TextCatalog {
    /// The item class of the [`Catalog`].
    type Class<'a>;

    /// The default class produced by the [`Catalog`].
    fn default<'a>() -> <Self as MovableListCatalog>::Class<'a>;

    /// The class of the remove button with the given status.
    fn remove_button_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a>;

    /// The class of a item.
    fn item_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a>;

    /// The class of a dropping empty slot.
    fn dropping_empty_slot_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a>;
}

pub type CloneableButtonStyleFn<'a, Theme> = Rc<dyn Fn(&Theme, ButtonStatus) -> ButtonStyle + 'a>;
pub type CloneableContainerStyleFn<'a, Theme> = Rc<dyn Fn(&Theme) -> ContainerStyle + 'a>;

pub struct MovableListStyleFns<'a, Theme> {
    remove_button_style_fn: CloneableButtonStyleFn<'a, Theme>,
    item_style_fn: CloneableContainerStyleFn<'a, Theme>,
    dropping_empty_slot_style_fn: CloneableContainerStyleFn<'a, Theme>,
}

impl<'a> Default for MovableListStyleFns<'a, iced::Theme> {
    fn default() -> Self {
        fn default_item_class(theme: &iced::Theme) -> ContainerStyle {
            let palette = theme.extended_palette();
            let mut border = border::rounded(32).width(2);
            border.color = palette.background.stronger.color;
            ContainerStyle {
                border,
                ..Default::default()
            }
        }

        fn default_dropping_empty_slot_class(theme: &iced::Theme) -> ContainerStyle {
            let palette = theme.extended_palette();
            let border = border::rounded(32).width(2);
            ContainerStyle {
                background: Some(palette.primary.weak.color.into()),
                border,
                ..Default::default()
            }
        }

        Self {
            remove_button_style_fn: Rc::new(button::button_text_danger_class),
            item_style_fn: Rc::new(default_item_class),
            dropping_empty_slot_style_fn: Rc::new(default_dropping_empty_slot_class),
        }
    }
}

impl MovableListCatalog for iced::Theme {
    type Class<'a> = MovableListStyleFns<'a, Self>;

    fn default<'a>() -> <Self as MovableListCatalog>::Class<'a> {
        Default::default()
    }

    fn remove_button_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a> {
        let f = Rc::clone(&class.remove_button_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>
    }

    fn item_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a> {
        let f = Rc::clone(&class.item_style_fn);

        Box::new(move |theme: &Self| f(theme)) as ContainerStyleFn<'a, Self>
    }

    fn dropping_empty_slot_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a> {
        let f = Rc::clone(&class.dropping_empty_slot_style_fn);

        Box::new(move |theme: &Self| f(theme)) as ContainerStyleFn<'a, Self>
    }
}

#[derive(Clone)]
enum MovableListInnerMessage<Message> {
    Drag(Point, usize),
    Drop(Point),
    Cancel,
    Remove(usize),
    OuterMessage(Message),
}

impl<Message> Debug for MovableListInnerMessage<Message> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("MovableListInnerMessage::")?;
        match self {
            Self::Drag(arg0, arg1) => f.debug_tuple("Drag").field(arg0).field(arg1).finish(),
            Self::Drop(arg0) => f.debug_tuple("Drop").field(arg0).finish(),
            Self::Cancel => f.write_str("Cancel"),
            Self::Remove(arg0) => f.debug_tuple("Remove").field(arg0).finish(),
            Self::OuterMessage(..) => f.write_str("OuterMessage"),
        }
    }
}

struct MovableListShimMut<'a, 'b, Id, Message> {
    ids: &'a [Option<Id>],
    horizontal: bool,
    on_drag: Option<&'a Box<dyn Fn(&Id, Point) -> Message + 'b>>,
    on_drop: Option<&'a Box<dyn Fn(&[&Id]) -> Message + 'b>>,
    on_remove: Option<&'a Box<dyn Fn(&Id) -> Message + 'b>>,
}

impl<'a, 'b, Id, Message> MovableListShimMut<'a, 'b, Id, Message> {
    fn update(
        &mut self,
        shell: &mut Shell<'_, Message>,
        layout_viewport: Option<(&Layout<'_>, &Rectangle)>,
        cursor: Cursor,
        state: &mut MovableListState,
        message: MovableListInnerMessage<Message>,
    ) {
        match message {
            MovableListInnerMessage::Drag(_position, idx) => {
                let Some((layout, viewport)) = layout_viewport else {
                    unreachable!("A drag event is happened on a overlay");
                };
                // NOTE the `position` is not correct inside a scrollable, use the position from
                // `cursor` instead
                // land the cursor to get the position
                let Some(position) = cursor.land().position() else {
                    tracing::warn!(
                        "The position of cursor isn't available when there is a drag event"
                    );
                    return;
                };
                let dragged_bounds = if let Some((dragged_idx, dragged_bounds)) = state.dragged {
                    if dragged_idx != idx {
                        let bounds = layout.child(idx).bounds();
                        state.dragged = Some((idx, layout.child(idx).bounds()));
                        shell.invalidate_layout();
                        bounds
                    } else {
                        dragged_bounds
                    }
                } else {
                    let bounds = layout.child(idx).bounds();
                    state.dragged = Some((idx, bounds));
                    shell.invalidate_layout();
                    bounds
                };
                if let Some(on_drag) = &self.on_drag {
                    let Some(id) = &self.ids[idx] else {
                        unreachable!("the slot[{idx}] isn't a MovableListSlot::Occupied");
                    };
                    shell.publish(on_drag(id, position));
                }
                let new_dropping = self.is_dropping(layout, &position, &dragged_bounds, viewport);
                if state.dropping != new_dropping {
                    state.dropping = new_dropping;
                    shell.invalidate_layout();
                }
            }
            MovableListInnerMessage::Drop(_position) => {
                let Some((layout, viewport)) = layout_viewport else {
                    unreachable!("A drag event is happened on a overlay");
                };
                // NOTE the `position` is not correct inside a scrollable, use the position from
                // `cursor` instead
                // land the cursor to get the position
                let Some(position) = cursor.land().position() else {
                    tracing::warn!(
                        "The position of cursor isn't available when there is a drag event"
                    );
                    return;
                };
                if let Some((dragged_idx, dragged_bounds)) = state.dragged {
                    state.dropping = self.is_dropping(layout, &position, &dragged_bounds, viewport);
                    if let Some(on_drop) = &self.on_drop
                        && let Some(dropping_idx) = state.dropping
                    {
                        shell.publish(on_drop(
                                &self
                                    .ids
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(idx, id)| {
                                        if idx == dropping_idx {
                                            let Some(id) = &self.ids[dragged_idx] else {
                                                unreachable!("the slot[{dragged_idx}] isn't a MovableListSlot::Occupied");
                                            };
                                            Some(id)
                                        } else if idx == dragged_idx {
                                            None
                                        } else {
                                            id.as_ref()
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            ));
                    }
                }
                if state.dragged.take().is_some() {
                    shell.invalidate_layout();
                }
                state.dropping.take();
            }
            MovableListInnerMessage::Cancel => {
                if let Some(on_drop) = &self.on_drop {
                    shell.publish(on_drop(&self.ids.iter().flatten().collect::<Vec<_>>()));
                }
                if state.dragged.take().is_some() {
                    shell.invalidate_layout();
                }
                state.dropping.take();
            }
            MovableListInnerMessage::Remove(idx) => {
                tracing::debug!("Remove slot[{idx}]");
                if let Some(on_remove) = &self.on_remove {
                    if let Some(id) = &self.ids[idx] {
                        shell.publish(on_remove(id));
                        shell.request_redraw();
                    } else {
                        unreachable!("the slot[{idx}] isn't a MovableListSlot::Occupied");
                    }
                }
            }
            MovableListInnerMessage::OuterMessage(message) => shell.publish(message),
        }
    }

    fn is_dropping(
        &self,
        layout: &Layout<'_>,
        position: &Point,
        dragged_slot_bounds: &Rectangle,
        viewport: &Rectangle,
    ) -> Option<usize> {
        let mut bounds = layout.bounds();
        // use the intersection of viewport to limit the area where we can drop the item.
        bounds = bounds.intersection(viewport).unwrap_or(bounds);
        tracing::debug!(
            "position: {position:?}, widget bounds: {:?}, viewport: {viewport:?}, intersection: {bounds:?}",
            layout.bounds()
        );
        if !bounds.contains(*position) {
            return None;
        }
        if self.horizontal {
            let mut max_idx = 0;
            let x_bound = position.x - dragged_slot_bounds.width / 2.;
            for idx in 1..self.ids.len() {
                if self.ids[idx].is_some() {
                    let slot_bounds = layout.child(idx).bounds();
                    if x_bound < slot_bounds.x {
                        return Some(idx - 1);
                    }
                    max_idx = max_idx.max(idx + 1);
                }
            }
            Some(max_idx)
        } else {
            let mut max_idx = 0;
            let y_bound = position.y - dragged_slot_bounds.height / 2.;
            for idx in 1..self.ids.len() {
                if self.ids[idx].is_some() {
                    let slot_bounds = layout.child(idx).bounds();
                    if y_bound < slot_bounds.y {
                        return Some(idx - 1);
                    }
                    max_idx = max_idx.max(idx + 1);
                }
            }
            Some(max_idx)
        }
    }
}

impl<'a, 'b, Id, Message, Renderer> Composer<Message, MovableListInnerMessage<Message>, Renderer>
    for (
        MovableListShimMut<'a, 'b, Id, Message>,
        &'a mut MovableListState,
    )
{
    fn compose(
        &mut self,
        _event: &Event,
        _layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        message: MovableListInnerMessage<Message>,
    ) {
        self.0.update(shell, None, cursor, self.1, message);
    }
}

/// A container of a list that its items can be dragged to reorder.
pub struct MovableList<'a, Id, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: MovableListCatalog,
{
    raw_children: Vec<Option<(Id, Element<'a, Message, Theme, Renderer>)>>,
    children: Vec<Element<'a, MovableListInnerMessage<Message>, Theme, Renderer>>,
    ids: Vec<Option<Id>>,
    empty_slot_layouts: Vec<Option<MovableListEmptySlotLayout>>,
    remove_button_text_size: Option<Pixels>,
    remove_button_size: Option<Size>,
    spacing: f32,
    item_padding: Option<Padding>,
    horizontal: bool,
    class: <Theme as MovableListCatalog>::Class<'a>,
    on_drag: Option<Box<dyn Fn(&Id, Point) -> Message + 'a>>,
    on_drop: Option<Box<dyn Fn(&[&Id]) -> Message + 'a>>,
    on_remove: Option<Box<dyn Fn(&Id) -> Message + 'a>>,
}

impl<'a, Id, Message, Theme, Renderer> MovableList<'a, Id, Message, Theme, Renderer>
where
    Id: 'a,
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    /// Creates a [`MovableList`].
    pub fn new() -> Self {
        Self {
            raw_children: vec![None],
            children: vec![],
            ids: vec![],
            empty_slot_layouts: vec![],
            remove_button_text_size: None,
            remove_button_size: None,
            spacing: 0.,
            item_padding: None,
            horizontal: true,
            class: <Theme as MovableListCatalog>::default(),
            on_drag: None,
            on_drop: None,
            on_remove: None,
        }
    }

    /// Sets the text size of remove button.
    pub fn remove_button_text_size(mut self, size: impl Into<Pixels>) -> Self {
        self.remove_button_text_size = Some(size.into());
        self
    }

    /// Sets the size of remove button.
    pub fn remove_button_size(mut self, size: impl Into<Size>) -> Self {
        self.remove_button_size = Some(size.into());
        self
    }

    /// Sets the padding of item container.
    pub fn item_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_padding = Some(padding.into());
        self
    }

    /// Sets the spacing between list items.
    pub fn spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        self.spacing = spacing.into().0;
        self
    }

    /// Lays items out horizontally.
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// Lays items out vertically.
    pub fn vertical(mut self) -> Self {
        self.horizontal = false;
        self
    }

    pub fn class(mut self, class: impl Into<<Theme as MovableListCatalog>::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    pub fn on_drag(mut self, on_drag: impl Fn(&Id, Point) -> Message + 'a) -> Self {
        self.on_drag = Some(Box::new(on_drag));
        self
    }

    pub fn on_drop(mut self, on_drop: impl Fn(&[&Id]) -> Message + 'a) -> Self {
        self.on_drop = Some(Box::new(on_drop));
        self
    }

    pub fn on_remove(mut self, on_remove: impl Fn(&Id) -> Message + 'a) -> Self {
        self.on_remove = Some(Box::new(on_remove));
        self
    }

    /// Push a item
    pub fn push(mut self, id: Id, item: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        self.raw_children.push(Some((id, item.into())));
        self.raw_children.push(None);
        self
    }

    fn seal(&mut self) {
        let mut idx = self.children.len();
        let len = self.children.len() + self.raw_children.len();
        for raw_child in self.raw_children.drain(..) {
            if let Some((id, element)) = raw_child {
                let mut element = element.map(MovableListInnerMessage::OuterMessage);
                element = if self.on_remove.is_some() {
                    let mut remove_button = ExtButton::new(text("x", self.remove_button_text_size))
                        .class(Theme::remove_button_class(&self.class))
                        .on_release_with(Some(move || MovableListInnerMessage::Remove(idx)));
                    if let Some(size) = self.remove_button_size {
                        remove_button = remove_button.width(size.width).height(size.height);
                    }
                    Row::new()
                        .align_y(Vertical::Center)
                        .push(element)
                        .push(remove_button)
                        .into()
                } else {
                    element
                };
                let mut container = Container::new(element).class(Theme::item_class(&self.class));
                if let Some(padding) = self.item_padding {
                    container = container.padding(padding);
                }
                element = if self.on_drop.is_some() {
                    Droppable::new(container)
                        .drag_center(true)
                        .drag_size(Size::new(0., 0.))
                        .drag_hide(true)
                        .on_drag({
                            move |position, _rectangle| MovableListInnerMessage::Drag(position, idx)
                        })
                        .on_drop(|position, _rectangle| MovableListInnerMessage::Drop(position))
                        .on_cancel(MovableListInnerMessage::Cancel)
                        .into()
                } else {
                    container.into()
                };
                self.children.push(element);
                self.empty_slot_layouts.push(None);
                self.ids.push(Some(id));
            } else {
                let layout = Self::empty_slot_layout(idx, len, None, None);
                self.children.push(Self::empty_slot_element(
                    self.horizontal,
                    self.spacing,
                    &self.class,
                    layout,
                ));
                self.empty_slot_layouts.push(Some(layout));
                self.ids.push(None);
            }
            idx += 1;
        }
    }

    fn empty_slot_layout(
        idx: usize,
        len: usize,
        dragged: Option<(usize, Rectangle)>,
        dropping: Option<usize>,
    ) -> MovableListEmptySlotLayout {
        let mut layout = MovableListEmptySlotLayout::new();
        // keep the same tree structure
        if let Some((dragged_idx, dragged_rectangle)) = dragged {
            if let Some(dropping_idx) = dropping {
                if dropping_idx == idx {
                    layout.middle = Some(dragged_rectangle);
                    layout.before = 2;
                    layout.after = 2;
                    if len == 3 {
                        // only one element
                        layout.remove_padding();
                    } else if idx == 0 {
                        // we are at the first empty slot. don't add padding before it
                        layout.before = 0;
                    } else if dragged_idx == 1 && dragged_idx + 1 == dropping_idx {
                        // the dragged element is the first one, and it is dropping at the empty
                        // slot right after it. don't add padding before it
                        layout.before = 0;
                    } else if idx + 1 == len {
                        // we are at the last empty slot. don't add padding after it
                        layout.after = 0;
                    } else if dropping_idx + 1 == dragged_idx && dragged_idx + 2 == len {
                        // the dragged element is the last one, and it is dropping at the empty
                        // slot right before it. don't add padding after it
                        layout.after = 0;
                    }
                } else {
                    if dragged_idx + 2 == len && idx + 1 == dragged_idx {
                        // if the dragged item is the last one, we set the empty slot before it to zero
                        // space
                        layout.remove_padding();
                    } else if idx == dragged_idx + 1 {
                        // otherwise, we set the empty slot after the dragged one to zero
                        layout.remove_padding();
                    } else if idx == 0 || idx + 1 == len {
                        layout.remove_padding();
                    }
                }
            } else {
                if dragged_idx + 2 == len && idx + 1 == dragged_idx {
                    // if the dragged item is the last one, we set the empty slot before it to zero
                    // space
                    layout.remove_padding();
                } else if idx == dragged_idx + 1 {
                    // otherwise, we set the empty slot after the dragged one to zero
                    layout.remove_padding();
                } else if idx == 0 || idx + 1 == len {
                    layout.remove_padding();
                }
            }
        } else {
            if idx == 0 || idx + 1 == len {
                layout.remove_padding();
            }
        }

        layout
    }

    fn empty_slot_element(
        horizontal: bool,
        spacing: f32,
        class: &<Theme as MovableListCatalog>::Class<'a>,
        layout: MovableListEmptySlotLayout,
    ) -> Element<'a, MovableListInnerMessage<Message>, Theme, Renderer> {
        let mut padding = Padding::ZERO;
        if horizontal {
            padding.left = spacing / 2. * layout.before as f32;
            padding.right = spacing / 2. * layout.after as f32;
        } else {
            padding.top = spacing / 2. * layout.before as f32;
            padding.bottom = spacing / 2. * layout.after as f32;
        }
        let middle = if let Some(middle) = layout.middle {
            Container::new(Space::new().width(middle.width).height(middle.height))
                .class(Theme::dropping_empty_slot_class(class))
        } else {
            Container::new(Space::new())
        };
        Container::new(middle).padding(padding).into()
    }

    fn split_mut<'b>(
        &'b mut self,
    ) -> (
        MovableListShimMut<'b, 'a, Id, Message>,
        &'b mut [Element<'a, MovableListInnerMessage<Message>, Theme, Renderer>],
    ) {
        (
            MovableListShimMut {
                ids: &self.ids,
                horizontal: self.horizontal,
                on_drag: self.on_drag.as_ref(),
                on_drop: self.on_drop.as_ref(),
                on_remove: self.on_remove.as_ref(),
            },
            &mut self.children,
        )
    }
}

impl<'a, Id, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MovableList<'a, Id, Message, Theme, Renderer>
where
    Id: 'a,
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MovableListState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MovableListState {
            dragged: None,
            dropping: None,
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state: &MovableListState = tree.state.downcast_ref();
        // update empty slot layouts
        for idx in 0..self.children.len() {
            if let Some(layout) = self.empty_slot_layouts[idx] {
                let new_layout = Self::empty_slot_layout(
                    idx,
                    self.children.len(),
                    state.dragged,
                    state.dropping,
                );
                if new_layout != layout {
                    tracing::debug!(
                        "New empty slot layout at {idx}, from [{layout:?}] to [{new_layout:?}]"
                    );
                    self.empty_slot_layouts[idx] = Some(new_layout);
                    self.children[idx] = Self::empty_slot_element(
                        self.horizontal,
                        self.spacing,
                        &self.class,
                        new_layout,
                    );
                }
            }
        }
        let axis = if self.horizontal {
            layout::flex::Axis::Horizontal
        } else {
            layout::flex::Axis::Vertical
        };
        layout::flex::resolve(
            axis,
            renderer,
            limits,
            Length::Shrink,
            Length::Shrink,
            Padding::ZERO,
            // spacing will be representing by empty slot
            0.,
            Alignment::Start,
            &mut self.children,
            &mut tree.children,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state: &MovableListState = tree.state.downcast_ref();
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .enumerate()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|(((idx, child), child_state), child_layout)| {
                    if !state.is_dragged(idx) && self.ids[idx].is_some() {
                        child.as_widget_mut().operate(
                            child_state,
                            child_layout,
                            renderer,
                            operation,
                        );
                    }
                });
        });
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
        let mut local_messages = vec![];
        let mut local_shell = shell.local(&mut local_messages);
        for (((idx, child), child_state), child_layout) in self
            .children
            .iter_mut()
            .enumerate()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            if self.ids[idx].is_some() {
                let is_captured = local_shell.is_event_captured();
                child.as_widget_mut().update(
                    child_state,
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    &mut local_shell,
                    viewport,
                );
                if !is_captured && local_shell.is_event_captured() {
                    tracing::debug!("Event{event:?} is captured by {idx}");
                }
            }
        }
        let state: &mut MovableListState = tree.state.downcast_mut();

        drop(local_shell);
        let (mut shim, _) = self.split_mut();
        for local_message in local_messages {
            shim.update(
                shell,
                Some((&layout, viewport)),
                cursor,
                state,
                local_message,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: MouseCursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> MouseInteraction {
        let state: &MovableListState = tree.state.downcast_ref();
        if state.dropping.is_some() {
            MouseInteraction::Grab
        } else if state.dragged.is_some() {
            MouseInteraction::Grabbing
        } else {
            self.children
                .iter()
                .enumerate()
                .zip(&tree.children)
                .zip(layout.children())
                .map(|(((idx, child), child_state), child_layout)| {
                    if !state.is_dragged(idx) && self.ids[idx].is_some() {
                        // only operates on non-dragged item
                        return child.as_widget().mouse_interaction(
                            child_state,
                            child_layout,
                            cursor,
                            viewport,
                            renderer,
                        );
                    }
                    MouseInteraction::None
                })
                .max()
                .unwrap_or_default()
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
        let state: &MovableListState = tree.state.downcast_ref();

        for (((idx, child), child_state), child_layout) in self
            .children
            .iter()
            .enumerate()
            .zip(&tree.children)
            .zip(layout.children())
            .filter(|(_, layout)| layout.bounds().intersects(viewport))
        {
            if !state.is_dragged(idx) {
                child.as_widget().draw(
                    child_state,
                    renderer,
                    theme,
                    renderer_style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let (shim, children) = self.split_mut();
        let children = children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((child, child_state), child_layout)| {
                child.as_widget_mut().overlay(
                    child_state,
                    child_layout,
                    renderer,
                    viewport,
                    translation,
                )
            })
            .collect::<Vec<_>>();

        let state: &mut MovableListState = tree.state.downcast_mut();
        (!children.is_empty()).then(|| {
            ComposerOverlay::overlay((shim, state), Group::with_children(children).overlay())
        })
    }
}

impl<'a, Id, Message, Theme, Renderer> From<MovableList<'a, Id, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Id: 'a,
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn from(mut widget: MovableList<'a, Id, Message, Theme, Renderer>) -> Self {
        widget.seal();
        Element::new(widget)
    }
}

#[derive(Clone)]
pub struct ScrollableMovableListMessage<Message>(ScrollableMovableListInnerMessage<Message>);

impl<Message> ScrollableMovableListMessage<Message> {
    pub fn wrap(m: Message) -> Self {
        Self(ScrollableMovableListInnerMessage::OuterMessage(m))
    }
}

impl<Message> Debug for ScrollableMovableListMessage<Message> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("ScrollableMovableListInnerMessage::")?;
        match &self.0 {
            ScrollableMovableListInnerMessage::Drag(..) => f.write_str("Drag"),
            ScrollableMovableListInnerMessage::Drop(..) => f.write_str("Drop"),
            ScrollableMovableListInnerMessage::OuterMessage(..) => f.write_str("OuterMessage"),
        }
    }
}

#[derive(Clone)]
enum ScrollableMovableListInnerMessage<Message> {
    Drag(Option<Message>),
    Drop(Message),
    OuterMessage(Message),
}

impl<Message> From<ScrollableMovableListInnerMessage<Message>>
    for ScrollableMovableListMessage<Message>
{
    fn from(value: ScrollableMovableListInnerMessage<Message>) -> Self {
        Self(value)
    }
}

/// Local state of the [`MovableList`].
#[derive(Default)]
struct ScrollableMovableListState {
    dragged: bool,
    last_frame: Option<Instant>,
    scroll_factor: Option<f32>,
}

struct ScrollableMovableListShimMut<Theme> {
    horizontal: bool,
    phantom: PhantomData<Theme>,
}

impl<'a, Message, Theme, Renderer>
    Composer<Message, ScrollableMovableListMessage<Message>, Renderer>
    for (
        ScrollableMovableListShimMut<Theme>,
        &mut ScrollableMovableListState,
    )
where
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog + ScrollableCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn compose(
        &mut self,
        _event: &Event,
        _layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        message: ScrollableMovableListMessage<Message>,
    ) {
        ScrollableMovableList::<'a, Message, Theme, Renderer>::inner_update(
            shell,
            None,
            cursor,
            self.0.horizontal,
            self.1,
            message,
        );
    }
}

/// A scrollable container of `MovableList`
pub struct ScrollableMovableList<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, ScrollableMovableListMessage<Message>, Theme, Renderer>,
    scrollable_id: Option<WidgetId>,
    horizontal: bool,
}

pub type CreateScrollableFn<'a, Message, Theme, Renderer> = dyn Fn(
        Element<'a, ScrollableMovableListMessage<Message>, Theme, Renderer>,
        WidgetId,
        bool,
    ) -> Result<
        Scrollable<'a, ScrollableMovableListMessage<Message>, Theme, Renderer>,
        Element<'a, ScrollableMovableListMessage<Message>, Theme, Renderer>,
    > + 'a;

impl<'a, Message, Theme, Renderer> ScrollableMovableList<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog + ScrollableCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    pub fn new<Id: 'a + Clone>(
        movable_list: MovableList<'a, Id, Message, Theme, Renderer>,
        scrollable: &CreateScrollableFn<'a, Message, Theme, Renderer>,
    ) -> Self {
        // convert callback
        let MovableList {
            raw_children,
            children: _children,
            ids: _ids,
            empty_slot_layouts: _empty_slot_layouts,
            remove_button_text_size,
            remove_button_size,
            spacing,
            item_padding,
            horizontal,
            class,
            on_drag,
            on_drop,
            on_remove,
        } = movable_list;
        let mut movable_list = MovableList {
            raw_children: raw_children
                .into_iter()
                .map(|child| {
                    child.map(|(id, element)| (id, element.map(ScrollableMovableListMessage::wrap)))
                })
                .collect(),
            children: vec![],
            ids: vec![],
            empty_slot_layouts: vec![],
            remove_button_text_size,
            remove_button_size,
            spacing,
            item_padding,
            horizontal,
            class,
            on_drag: None,
            on_drop: None,
            on_remove: None,
        };
        movable_list = movable_list.on_drag(move |id, position| {
            ScrollableMovableListInnerMessage::Drag(on_drag.as_ref().map(|f| f(id, position)))
                .into()
        });
        if let Some(on_drop) = on_drop {
            movable_list = movable_list
                .on_drop(move |ids| ScrollableMovableListInnerMessage::Drop(on_drop(ids)).into());
        };
        if let Some(on_remove) = on_remove {
            movable_list = movable_list.on_remove(move |id| {
                ScrollableMovableListInnerMessage::OuterMessage(on_remove(id)).into()
            });
        };
        let scrollable_id = WidgetId::unique();
        match scrollable(
            Container::new(movable_list).center_x(Length::Fill).into(),
            scrollable_id.clone(),
            horizontal,
        ) {
            Ok(scrollable) => Self {
                content: scrollable.id(scrollable_id.clone()).into(),
                scrollable_id: Some(scrollable_id),
                horizontal,
            },
            Err(content) => Self {
                content,
                scrollable_id: None,
                horizontal,
            },
        }
    }

    fn calculate_scroll_factor(
        horizontal: bool,
        bounds: &Rectangle,
        position: &Point,
    ) -> Option<f32> {
        if horizontal {
            // don't scroll if the pointer is within the widget's horizontal bounds
            if position.x >= bounds.x && position.x <= bounds.x + bounds.width {
                return None;
            }
            if position.x < bounds.x {
                Some(position.x - bounds.x)
            } else {
                Some(position.x - bounds.x - bounds.width)
            }
        } else {
            // don't scroll if the pointer is within the widget's vertical bounds
            if position.y >= bounds.y || position.y <= bounds.y + bounds.height {
                return None;
            }
            if position.y < bounds.y {
                Some(position.y - bounds.y)
            } else {
                Some(position.y - bounds.y - bounds.height)
            }
        }
    }

    fn inner_update(
        shell: &mut Shell<'_, Message>,
        layout: Option<&Layout<'_>>,
        cursor: Cursor,
        horizontal: bool,
        state: &mut ScrollableMovableListState,
        message: ScrollableMovableListMessage<Message>,
    ) {
        match message.0 {
            ScrollableMovableListInnerMessage::Drag(m) => {
                tracing::debug!("dragging");
                let Some(layout) = layout else {
                    unreachable!("A drag event is happened on a overlay");
                };
                if !state.dragged {
                    state.dragged = true;
                    state.last_frame = None;
                    state.scroll_factor = None;
                } else if let Some(position) = cursor.land().position() {
                    // land the cursor to get the position
                    state.scroll_factor =
                        Self::calculate_scroll_factor(horizontal, &layout.bounds(), &position);
                    if state.scroll_factor.is_none() {
                        state.last_frame = None;
                    }
                } else {
                    tracing::warn!("Unable to get the position of the cursor");
                }
                if let Some(m) = m {
                    shell.publish(m)
                }
            }
            ScrollableMovableListInnerMessage::Drop(m) => {
                tracing::debug!("dropped");
                state.dragged = false;
                state.last_frame = None;
                state.scroll_factor = None;
                shell.publish(m)
            }
            ScrollableMovableListInnerMessage::OuterMessage(m) => shell.publish(m),
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ScrollableMovableList<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog + ScrollableCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ScrollableMovableListState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ScrollableMovableListState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(slice::from_ref(&self.content));
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
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut local_messages = vec![];
        let mut local_shell = shell.local(&mut local_messages);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local_shell,
            viewport,
        );
        drop(local_shell);

        let state: &mut ScrollableMovableListState = tree.state.downcast_mut();
        for local_message in local_messages {
            Self::inner_update(
                shell,
                Some(&layout),
                cursor,
                self.horizontal,
                state,
                local_message,
            );
        }
        if let Some(scroll_factor) = state.scroll_factor
            && let Some(scrollable_id) = &self.scrollable_id
            && let Event::Window(WindowEvent::RedrawRequested(now)) = event
        {
            const MAX_DELTA: u64 = 8;
            // like auto scrolling in `Scrollable`, use time as a factor
            let next = if let Some(last_frame) = state.last_frame {
                // 120Hz ?
                if (*now - last_frame).as_millis() >= MAX_DELTA as u128 {
                    state.last_frame = Some(*now);
                    // scroll
                    let offset = scroll_factor * (*now - last_frame).as_secs_f32();
                    tracing::debug!("Scroll by {offset}");
                    self.operate(
                        tree,
                        layout,
                        renderer,
                        &mut scrollable::scroll_by(
                            scrollable_id.clone(),
                            AbsoluteOffset {
                                x: offset,
                                y: offset,
                            },
                        ),
                    );
                    now.checked_add(Duration::from_millis(MAX_DELTA))
                } else {
                    last_frame.checked_add(Duration::from_millis(MAX_DELTA))
                }
            } else {
                state.last_frame = Some(*now);
                now.checked_add(Duration::from_millis(MAX_DELTA))
            };
            if let Some(next) = next {
                shell.request_redraw_at(next);
            } else {
                tracing::error!("Unable to get the next redraw instant: {now:?}");
            }
        }
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

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state: &mut ScrollableMovableListState = tree.state.downcast_mut();
        self.content
            .as_widget_mut()
            .overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
            .map(|o| {
                ComposerOverlay::overlay(
                    (
                        ScrollableMovableListShimMut::<Theme> {
                            horizontal: self.horizontal,
                            phantom: Default::default(),
                        },
                        state,
                    ),
                    o,
                )
            })
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
}

impl<'a, Message, Theme, Renderer> From<ScrollableMovableList<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog + ScrollableCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn from(widget: ScrollableMovableList<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

pub trait AdvancedMovableListCatalog:
    MovableListCatalog + ScrollableCatalog + TextInputCatalog + PickListCatalog
{
    /// The item class of the [`Catalog`].
    type Class<'a>;

    /// The default class produced by the [`Catalog`].
    fn default<'a>() -> <Self as AdvancedMovableListCatalog>::Class<'a>;

    /// The class of lv1 buttons.
    fn lv1_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a>;

    /// The class of the lv2 add button.
    fn lv2_add_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a>;

    /// The class of the lv2 done button.
    fn lv2_done_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a>;

    /// The class of the lv2 cancel button.
    fn lv2_cancel_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a>;

    /// The class of the text input.
    fn text_input_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as TextInputCatalog>::Class<'a>;

    /// The class of the pick list.
    fn pick_list_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as PickListCatalog>::Class<'a>;
}

pub type CloneableTextInputStyleFn<'a, Theme> =
    Rc<dyn Fn(&Theme, TextInputStatus) -> TextInputStyle + 'a>;
pub type CloneablePickListStyleFn<'a, Theme> =
    Rc<dyn Fn(&Theme, PickListStatus) -> PickListStyle + 'a>;

pub struct AdvancedMovableListStyleFns<'a, Theme> {
    lv1_button_style_fn: CloneableButtonStyleFn<'a, Theme>,
    lv2_add_button_style_fn: CloneableButtonStyleFn<'a, Theme>,
    lv2_done_button_style_fn: CloneableButtonStyleFn<'a, Theme>,
    lv2_cancel_button_style_fn: CloneableButtonStyleFn<'a, Theme>,
    text_input_style_fn: CloneableTextInputStyleFn<'a, Theme>,
    pick_list_style_fn: CloneablePickListStyleFn<'a, Theme>,
}

impl Default for AdvancedMovableListStyleFns<'_, iced::Theme> {
    fn default() -> Self {
        Self {
            lv1_button_style_fn: Rc::new(button::button_class),
            lv2_add_button_style_fn: Rc::new(button::button_class),
            lv2_done_button_style_fn: Rc::new(button::button_class),
            lv2_cancel_button_style_fn: Rc::new(button::button_danger_class),
            text_input_style_fn: Rc::new(text_input::default),
            pick_list_style_fn: Rc::new(pick_list::default),
        }
    }
}

impl AdvancedMovableListCatalog for iced::Theme {
    type Class<'a> = AdvancedMovableListStyleFns<'a, Self>;

    fn default<'a>() -> <Self as AdvancedMovableListCatalog>::Class<'a> {
        Default::default()
    }

    fn lv1_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a> {
        let f = Rc::clone(&class.lv1_button_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>
    }

    fn lv2_add_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a> {
        let f = Rc::clone(&class.lv2_add_button_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>
    }

    fn lv2_done_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a> {
        let f = Rc::clone(&class.lv2_done_button_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>
    }

    fn lv2_cancel_button_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as ButtonCatalog>::Class<'a> {
        let f = Rc::clone(&class.lv2_cancel_button_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>
    }

    fn text_input_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as TextInputCatalog>::Class<'a> {
        let f = Rc::clone(&class.text_input_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as TextInputStyleFn<'a, Self>
    }

    fn pick_list_class<'a>(
        class: &<Self as AdvancedMovableListCatalog>::Class<'a>,
    ) -> <Self as PickListCatalog>::Class<'a> {
        let f = Rc::clone(&class.pick_list_style_fn);

        Box::new(move |theme: &Self, status| f(theme, status)) as PickListStyleFn<'a, Self>
    }
}

#[derive(Clone)]
pub struct AdvancedMovableListMessage<Message>(AdvancedMovableListInnerMessage<Message>);

impl<Message> AdvancedMovableListMessage<Message> {
    pub fn wrap(m: Message) -> Self {
        Self(AdvancedMovableListInnerMessage::OuterMessage(m))
    }
}

impl<Message> Debug for AdvancedMovableListMessage<Message> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("AdvancedMovableListInnerMessage::")?;
        match &self.0 {
            AdvancedMovableListInnerMessage::OuterMessage(..) => f.write_str("OuterMessage"),
            AdvancedMovableListInnerMessage::EnableAddMode => f.write_str("EnableAddMode"),
            AdvancedMovableListInnerMessage::EnableEditMode => f.write_str("EnableEditMode"),
            AdvancedMovableListInnerMessage::UpdateAddText(s) => {
                f.debug_tuple("UpdateAddText").field(s).finish()
            }
            AdvancedMovableListInnerMessage::AddItem => f.write_str("AddItem"),
            AdvancedMovableListInnerMessage::Cancel => f.write_str("Cancel"),
            AdvancedMovableListInnerMessage::Done => f.write_str("Done"),
            AdvancedMovableListInnerMessage::RemoveItem(s) => {
                f.debug_tuple("RemoveItem").field(s).finish()
            }
            AdvancedMovableListInnerMessage::ReorderItems(items) => {
                f.debug_tuple("ReorderItems").field(items).finish()
            }
        }
    }
}

#[derive(Clone)]
enum AdvancedMovableListInnerMessage<Message> {
    EnableAddMode,
    EnableEditMode,
    UpdateAddText(String),
    AddItem,
    Cancel,
    Done,
    RemoveItem(String),
    ReorderItems(Vec<String>),
    OuterMessage(Message),
}

impl<Message> From<AdvancedMovableListInnerMessage<Message>>
    for AdvancedMovableListMessage<Message>
{
    fn from(value: AdvancedMovableListInnerMessage<Message>) -> Self {
        Self(value)
    }
}

#[derive(Default, Debug)]
enum AdvancedMovableListState {
    #[default]
    Init,
    Add {
        new_items: Vec<String>,
        add_text: Option<String>,
    },
    Edit {
        new_order: Vec<usize>,
    },
}

struct AdvancedMovableListShimMut<'a, 'b, Id, Message>
where
    'b: 'a,
{
    text_to_id: Option<&'a Box<dyn Fn(&String) -> Option<Id> + 'b>>,
    selections: &'a mut [(Id, String)],
    children_flag: &'a mut [bool],
    on_done: Option<&'a Rc<dyn Fn(&[Id]) -> Message + 'b>>,
}

impl<'a, 'b, Id, Message> AdvancedMovableListShimMut<'a, 'b, Id, Message>
where
    'b: 'a,
    Id: Clone,
{
    fn request_build_movable_list(&mut self) {
        self.children_flag[0] = true;
    }

    fn request_build_control_row(&mut self) {
        self.children_flag[1] = true;
    }

    fn request_redraw(
        &mut self,
        shell: &mut Shell<'_, Message>,
        build_movable_list: bool,
        build_control_row: bool,
    ) {
        if build_movable_list {
            self.request_build_movable_list();
        }
        if build_control_row {
            self.request_build_control_row();
        }
        shell.invalidate_layout();
        shell.request_redraw();
    }

    fn update(
        &mut self,
        shell: &mut Shell<'_, Message>,
        state: &mut AdvancedMovableListState,
        message: AdvancedMovableListInnerMessage<Message>,
    ) {
        match message {
            AdvancedMovableListInnerMessage::EnableAddMode => {
                *state = AdvancedMovableListState::Add {
                    new_items: vec![],
                    add_text: None,
                };
                self.request_redraw(shell, true, true);
            }
            AdvancedMovableListInnerMessage::EnableEditMode => {
                *state = AdvancedMovableListState::Edit {
                    new_order: (0..self.selections.len()).collect(),
                };
                self.request_redraw(shell, true, true);
            }
            AdvancedMovableListInnerMessage::UpdateAddText(s) => {
                if let AdvancedMovableListState::Add { add_text, .. } = state {
                    tracing::debug!("Replace {add_text:?} with {s}");
                    *add_text = Some(s);
                    self.request_redraw(shell, false, true);
                } else {
                    tracing::warn!("It isn't in the add mode, it's {state:?}");
                }
            }
            AdvancedMovableListInnerMessage::AddItem => {
                if let AdvancedMovableListState::Add {
                    new_items,
                    add_text,
                } = state
                {
                    tracing::debug!("Add {add_text:?}");
                    if let Some(add_text) = add_text.take()
                        && !add_text.is_empty()
                    {
                        new_items.push(add_text);
                        self.request_redraw(shell, true, true);
                    }
                } else {
                    tracing::warn!("It isn't in the add mode, it's {state:?}");
                }
            }
            AdvancedMovableListInnerMessage::Cancel => {
                tracing::debug!("Cancel from state[{state:?}]");
                *state = AdvancedMovableListState::Init;
                self.request_redraw(shell, true, true);
            }
            AdvancedMovableListInnerMessage::Done => {
                let ids: Vec<Id> = match state {
                    AdvancedMovableListState::Init => {
                        return;
                    }
                    AdvancedMovableListState::Add { new_items, .. } => {
                        if let Some(text_to_id) = &self.text_to_id {
                            self.selections
                                .iter()
                                .map(|i| &i.1)
                                .chain(new_items.iter())
                                .filter_map(|s| {
                                    let id = text_to_id(s);
                                    if id.is_none() {
                                        tracing::warn!("Unable to get id of text[{s}]");
                                    }
                                    id
                                })
                                .collect()
                        } else {
                            unreachable!("It shouldn't enter add mode if there is no `text_to_id`");
                        }
                    }
                    AdvancedMovableListState::Edit { new_order } => {
                        let mut ids = Vec::with_capacity(new_order.len());
                        for idx in new_order {
                            if let Some(selection) = self.selections.get(*idx) {
                                ids.push(selection.0.clone());
                            } else {
                                tracing::error!("Unable to get the {idx} selection");
                            }
                        }
                        ids
                    }
                };
                if let Some(on_done) = &self.on_done {
                    shell.publish(on_done(&ids));
                }
                *state = AdvancedMovableListState::Init;
                self.request_redraw(shell, true, true);
            }
            AdvancedMovableListInnerMessage::RemoveItem(item) => {
                if let AdvancedMovableListState::Edit { new_order } = state {
                    tracing::debug!("Remove {item}");
                    if let Some(pos) = self.selections.iter().position(|s| s.1 == item) {
                        if let Some(new_order_pos) = new_order.iter().position(|n| *n == pos) {
                            new_order.remove(new_order_pos);
                            self.request_redraw(shell, true, false);
                        } else {
                            tracing::warn!(
                                "The position of item[{item}:{pos}] doesn't exist in `new_order`"
                            );
                        }
                    } else {
                        tracing::warn!("The item[{item}] isn't found");
                    }
                } else {
                    tracing::warn!("It isn't in the add mode, it's {state:?}");
                }
            }
            AdvancedMovableListInnerMessage::ReorderItems(items) => {
                if let AdvancedMovableListState::Edit { new_order } = state {
                    tracing::debug!("Reorder {items:?}");
                    *new_order = Vec::with_capacity(items.len());
                    for item in items {
                        if let Some(pos) = self.selections.iter().position(|s| s.1 == item) {
                            new_order.push(pos);
                        } else {
                            tracing::warn!("The item[{item}] doesn't found");
                        }
                    }
                    self.request_redraw(shell, true, false);
                } else {
                    tracing::warn!("It isn't in the add mode, it's {state:?}");
                }
            }
            AdvancedMovableListInnerMessage::OuterMessage(m) => shell.publish(m),
        }
    }
}

impl<'a, 'b, Id, Message, Renderer> Composer<Message, AdvancedMovableListMessage<Message>, Renderer>
    for (
        AdvancedMovableListShimMut<'a, 'b, Id, Message>,
        &'a mut AdvancedMovableListState,
    )
where
    Id: Clone,
{
    fn compose(
        &mut self,
        _event: &Event,
        _layout: Layout<'_>,
        _cursor: Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        message: AdvancedMovableListMessage<Message>,
    ) {
        self.0.update(shell, self.1, message.0);
    }
}

/// An advanced version of `MovableList`
pub struct AdvancedMovableList<'a, Id, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: AdvancedMovableListCatalog,
    Renderer: TextRenderer,
{
    create_movable_list: Box<dyn Fn() -> MovableList<'a, Id, Message, Theme, Renderer> + 'a>,
    create_scrollable:
        Rc<CreateScrollableFn<'a, AdvancedMovableListMessage<Message>, Theme, Renderer>>,
    text_to_id: Option<Box<dyn Fn(&String) -> Option<Id> + 'a>>,
    id_to_element: Option<Box<dyn Fn(&Id) -> Element<'a, Message, Theme, Renderer> + 'a>>,
    variants: Option<Vec<Id>>,
    selections: Vec<(Id, String)>,
    children: Vec<Element<'a, AdvancedMovableListMessage<Message>, Theme, Renderer>>,
    children_flag: [bool; 2],
    movable_list_height: Option<f32>,
    control_row_spacing: f32,
    control_row_text_size: Option<Pixels>,
    control_row_button_padding: Padding,
    control_row_height: Option<f32>,
    class: <Theme as AdvancedMovableListCatalog>::Class<'a>,
    on_done: Option<Rc<dyn Fn(&[Id]) -> Message + 'a>>,
}

impl<'a, Id, Message, Theme, Renderer> AdvancedMovableList<'a, Id, Message, Theme, Renderer>
where
    Id: 'a + ToString + Clone,
    Message: 'a + Clone,
    Theme: 'a + AdvancedMovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    /// Creates a [`AdvancedMovableList`].
    pub fn new(
        create_movable_list: impl Fn() -> MovableList<'a, Id, Message, Theme, Renderer> + 'a,
        create_scrollable: Rc<
            CreateScrollableFn<'a, AdvancedMovableListMessage<Message>, Theme, Renderer>,
        >,
    ) -> Self {
        Self {
            create_movable_list: Box::new(create_movable_list),
            create_scrollable,
            text_to_id: None,
            id_to_element: None,
            variants: None,
            selections: vec![],
            children: Vec::with_capacity(2),
            children_flag: [true, true],
            movable_list_height: None,
            control_row_spacing: 0.,
            control_row_text_size: None,
            control_row_button_padding: Padding::ZERO,
            control_row_height: None,
            class: <Theme as AdvancedMovableListCatalog>::default(),
            on_done: None,
        }
    }

    pub fn text_to_id(mut self, text_to_id: impl Fn(&String) -> Option<Id> + 'a) -> Self {
        self.text_to_id = Some(Box::new(text_to_id));
        self
    }

    pub fn id_to_element(
        mut self,
        id_to_element: impl Fn(&Id) -> Element<'a, Message, Theme, Renderer> + 'a,
    ) -> Self {
        self.id_to_element = Some(Box::new(id_to_element));
        self
    }

    pub fn variants(mut self, variants: Vec<Id>) -> Self {
        self.variants = Some(variants);
        self
    }

    pub fn movable_list_height(mut self, movable_list_height: f32) -> Self {
        self.movable_list_height = Some(movable_list_height);
        self
    }

    pub fn control_row_spacing(mut self, control_row_spacing: impl Into<Pixels>) -> Self {
        self.control_row_spacing = control_row_spacing.into().0;
        self
    }

    pub fn control_row_text_size(mut self, control_row_text_size: impl Into<Pixels>) -> Self {
        self.control_row_text_size = Some(control_row_text_size.into());
        self
    }

    pub fn control_row_button_padding(
        mut self,
        control_row_button_padding: impl Into<Padding>,
    ) -> Self {
        self.control_row_button_padding = control_row_button_padding.into();
        self
    }

    pub fn control_row_height(mut self, control_row_height: f32) -> Self {
        self.control_row_height = Some(control_row_height);
        self
    }

    pub fn on_done(mut self, on_done: impl Fn(&[Id]) -> Message + 'a) -> Self {
        self.on_done = Some(Rc::new(on_done));
        self
    }

    fn build_movable_list(&mut self, state: &mut AdvancedMovableListState) -> bool {
        if !self.children_flag[0] {
            return false;
        }

        let MovableList {
            raw_children,
            children: _children,
            ids: _ids,
            empty_slot_layouts: _empty_slot_layouts,
            remove_button_text_size,
            remove_button_size,
            spacing,
            item_padding,
            horizontal,
            class,
            on_drag: _,
            on_drop: _,
            on_remove: _,
        } = (self.create_movable_list)();

        let mut raw_children: Vec<_> = raw_children.into_iter().flatten().collect();
        self.selections = raw_children
            .iter()
            .map(|i| (i.0.clone(), i.0.to_string()))
            .collect();
        let mut edit_mode = false;
        match state {
            AdvancedMovableListState::Init => {}
            AdvancedMovableListState::Add { new_items, .. } => {
                if let Some(text_to_id) = &self.text_to_id
                    && let Some(id_to_element) = &self.id_to_element
                {
                    self.selections.reserve(new_items.len());
                    for new_item in new_items {
                        let Some(new_id) = text_to_id(new_item) else {
                            continue;
                        };
                        self.selections.push((new_id.clone(), new_item.clone()));
                        let new_element = id_to_element(&new_id);
                        raw_children.push((new_id, new_element));
                    }
                }
            }
            AdvancedMovableListState::Edit { new_order } => {
                edit_mode = true;
                let mut slots: Vec<_> = raw_children.into_iter().map(Some).collect();
                raw_children = Vec::with_capacity(new_order.len());
                let mut idx = 0;
                while idx < new_order.len() {
                    let slot_idx = new_order[idx];
                    if slot_idx >= slots.len() {
                        tracing::error!(
                            "the slot_idx[{slot_idx}] in `new_order` is out of bound, the length is {}",
                            slots.len()
                        );
                        break;
                    }
                    if let Some(item) = slots[slot_idx].take() {
                        raw_children.push(item);
                    } else {
                        tracing::error!("duplicated slot_idx[{slot_idx}] in `new_order`",);
                        break;
                    }
                    idx += 1;
                }
                if idx < new_order.len() {
                    // revert the items
                    while idx > 0 {
                        let slot_idx = new_order[idx - 1];
                        slots[slot_idx] = Some(raw_children.pop().expect("It shouldn't be empty"));
                        idx -= 1;
                    }
                    raw_children = slots.into_iter().flatten().collect();
                    // revert to AdvancedMovableListState::Init
                    *state = AdvancedMovableListState::Init;
                }
            }
        }

        let mut movable_list = MovableList::new().spacing(spacing).class(class);
        if let Some(remove_button_text_size) = remove_button_text_size {
            movable_list = movable_list.remove_button_text_size(remove_button_text_size);
        }
        if let Some(remove_button_size) = remove_button_size {
            movable_list = movable_list.remove_button_size(remove_button_size);
        }
        if let Some(item_padding) = item_padding {
            movable_list = movable_list.item_padding(item_padding);
        }
        if horizontal {
            movable_list = movable_list.horizontal();
        } else {
            movable_list = movable_list.vertical();
        }
        for raw_child in raw_children {
            movable_list = movable_list.push(
                raw_child.0,
                raw_child.1.map(AdvancedMovableListMessage::wrap),
            );
        }

        if edit_mode && self.on_done.is_some() {
            movable_list = movable_list
                .on_drop(|ids| {
                    AdvancedMovableListInnerMessage::ReorderItems(
                        ids.iter().map(|id| (*id).to_string()).collect(),
                    )
                    .into()
                })
                .on_remove(|id| AdvancedMovableListInnerMessage::RemoveItem(id.to_string()).into());
        }

        let content = Container::new(ScrollableMovableList::new(
            movable_list,
            self.create_scrollable.clone().as_ref(),
        ))
        .center_y(
            self.movable_list_height
                .map(Length::Fixed)
                .unwrap_or(Length::Shrink),
        )
        .into();
        if self.children.is_empty() {
            self.children.push(content);
        } else {
            self.children[0] = content;
        }

        self.children_flag[0] = false;
        true
    }

    fn build_control_row(&mut self, state: &AdvancedMovableListState) -> bool {
        if !self.children_flag[1] || self.children.is_empty() {
            // check if movable_list is built
            return false;
        }

        let mut row = Row::new()
            .spacing(self.control_row_spacing)
            .align_y(Vertical::Center);
        match state {
            AdvancedMovableListState::Init => {
                if self.text_to_id.is_some() && self.id_to_element.is_some() {
                    row = row.push(
                        ExtButton::new(text("Add", self.control_row_text_size))
                            .class(Theme::lv1_button_class(&self.class))
                            .padding(self.control_row_button_padding)
                            .on_release_with(Some(|| {
                                AdvancedMovableListInnerMessage::EnableAddMode.into()
                            })),
                    )
                }
                row = row.push(
                    ExtButton::new(text("Edit", self.control_row_text_size))
                        .class(Theme::lv1_button_class(&self.class))
                        .padding(self.control_row_button_padding)
                        .on_release_with(Some(|| {
                            AdvancedMovableListInnerMessage::EnableEditMode.into()
                        })),
                )
            }
            AdvancedMovableListState::Add {
                new_items,
                add_text,
            } => {
                let add_text = if let Some(variants) = &self.variants {
                    let selections: HashSet<_> = self.selections.iter().map(|i| &i.1).collect();
                    let variants: Vec<_> = variants
                        .iter()
                        .map(ToString::to_string)
                        .filter(|v| !selections.contains(v) && !new_items.contains(v))
                        .collect();
                    Row::new()
                        .spacing(self.control_row_spacing / 2.)
                        .align_y(Vertical::Center)
                        .push(
                            PickList::new(variants, add_text.clone(), |s| {
                                AdvancedMovableListInnerMessage::UpdateAddText(s).into()
                            })
                            .class(Theme::pick_list_class(&self.class)),
                        )
                        .push(
                            ExtButton::new(text("Add", self.control_row_text_size))
                                .class(Theme::lv2_add_button_class(&self.class))
                                .padding(self.control_row_button_padding)
                                .on_release_with(Some(|| {
                                    AdvancedMovableListInnerMessage::AddItem.into()
                                })),
                        )
                } else if self.text_to_id.is_some() {
                    Row::new()
                        .spacing(self.control_row_spacing / 2.)
                        .push(
                            TextInput::new("", add_text.as_deref().unwrap_or(""))
                                .class(Theme::text_input_class(&self.class))
                                .on_input(|s| {
                                    AdvancedMovableListInnerMessage::UpdateAddText(s).into()
                                })
                                .on_paste(|s| {
                                    AdvancedMovableListInnerMessage::UpdateAddText(s).into()
                                })
                                .on_submit(AdvancedMovableListInnerMessage::AddItem.into()),
                        )
                        .push(
                            ExtButton::new(text("Add", self.control_row_text_size))
                                .class(Theme::lv2_add_button_class(&self.class))
                                .padding(self.control_row_button_padding)
                                .on_release_with(Some(|| {
                                    AdvancedMovableListInnerMessage::AddItem.into()
                                })),
                        )
                } else {
                    unreachable!("There should't be any add mode");
                };
                row = row
                    .push(add_text)
                    .push(
                        ExtButton::new(text("Done", self.control_row_text_size))
                            .class(Theme::lv2_done_button_class(&self.class))
                            .padding(self.control_row_button_padding)
                            .on_release_with(Some(|| AdvancedMovableListInnerMessage::Done.into())),
                    )
                    .push(
                        ExtButton::new(text("Cancel", self.control_row_text_size))
                            .class(Theme::lv2_cancel_button_class(&self.class))
                            .padding(self.control_row_button_padding)
                            .on_release_with(Some(|| {
                                AdvancedMovableListInnerMessage::Cancel.into()
                            })),
                    );
            }
            AdvancedMovableListState::Edit { .. } => {
                row = row
                    .push(
                        ExtButton::new(text("Done", self.control_row_text_size))
                            .class(Theme::lv2_done_button_class(&self.class))
                            .padding(self.control_row_button_padding)
                            .on_release_with(Some(|| AdvancedMovableListInnerMessage::Done.into())),
                    )
                    .push(
                        ExtButton::new(text("Cancel", self.control_row_text_size))
                            .class(Theme::lv2_cancel_button_class(&self.class))
                            .padding(self.control_row_button_padding)
                            .on_release_with(Some(|| {
                                AdvancedMovableListInnerMessage::Cancel.into()
                            })),
                    );
            }
        };

        let content = Container::new(row)
            .center_y(
                self.control_row_height
                    .map(Length::Fixed)
                    .unwrap_or(Length::Shrink),
            )
            .into();

        if self.children.len() == 1 {
            self.children.push(content);
        } else {
            self.children[1] = content;
        }

        self.children_flag[1] = false;
        true
    }

    fn split_mut<'b>(
        &'b mut self,
    ) -> (
        AdvancedMovableListShimMut<'b, 'a, Id, Message>,
        &'b mut [Element<'a, AdvancedMovableListMessage<Message>, Theme, Renderer>],
    ) {
        (
            AdvancedMovableListShimMut {
                text_to_id: self.text_to_id.as_ref(),
                selections: &mut self.selections,
                children_flag: &mut self.children_flag,
                on_done: self.on_done.as_ref(),
            },
            &mut self.children,
        )
    }
}

impl<'a, Id, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for AdvancedMovableList<'a, Id, Message, Theme, Renderer>
where
    Id: 'a + ToString + Clone,
    Message: 'a + Clone,
    Theme: 'a + AdvancedMovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<AdvancedMovableListState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(AdvancedMovableListState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((row, row_state), row_layout)| {
                    row.as_widget_mut()
                        .operate(row_state, row_layout, renderer, operation)
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut local_messages = vec![];
        let mut local_shell = shell.local(&mut local_messages);
        self.children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .for_each(|((row, row_state), row_layout)| {
                row.as_widget_mut().update(
                    row_state,
                    event,
                    row_layout,
                    cursor,
                    renderer,
                    clipboard,
                    &mut local_shell,
                    viewport,
                )
            });
        drop(local_shell);

        let state: &mut AdvancedMovableListState = tree.state.downcast_mut();
        let (mut shim, _) = self.split_mut();
        for local_message in local_messages {
            shim.update(shell, state, local_message.0);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((row, row_state), row_layout)| {
                row.as_widget()
                    .mouse_interaction(row_state, row_layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or(Interaction::None)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let (shim, children) = self.split_mut();
        let children = children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((child, state), layout)| {
                child
                    .as_widget_mut()
                    .overlay(state, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();

        let state: &mut AdvancedMovableListState = tree.state.downcast_mut();
        (!children.is_empty()).then(|| {
            ComposerOverlay::overlay((shim, state), Group::with_children(children).overlay())
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state: &mut AdvancedMovableListState = tree.state.downcast_mut();
        let mut tree_diff = self.build_movable_list(state);
        if self.on_done.is_some() {
            // NOTE run `self.build_control_row` first
            tree_diff = self.build_control_row(state) || tree_diff;
        }
        if tree_diff {
            self.diff(tree);
            tracing::error!("The len of children: {}", self.children.len());
        }
        layout::flex::resolve(
            layout::flex::Axis::Vertical,
            renderer,
            limits,
            Length::Shrink,
            Length::Shrink,
            Padding::ZERO,
            0.,
            Alignment::Start,
            &mut self.children,
            &mut tree.children,
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((row, row_state), row_layout)| {
                row.as_widget().draw(
                    row_state,
                    renderer,
                    theme,
                    renderer_style,
                    row_layout,
                    cursor,
                    viewport,
                )
            })
            .count();
    }
}

impl<'a, Id, Message, Theme, Renderer> From<AdvancedMovableList<'a, Id, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Id: 'a + ToString + Clone,
    Message: 'a + Clone,
    Theme: 'a + AdvancedMovableListCatalog,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn from(widget: AdvancedMovableList<'a, Id, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

fn text<'a, Theme, Renderer>(s: &'a str, text_size: Option<Pixels>) -> Text<'a, Theme, Renderer>
where
    Theme: 'a + TextCatalog,
    Renderer: 'a + TextRenderer,
{
    let text = Text::new(s).center();
    if let Some(text_size) = text_size {
        text.size(text_size)
    } else {
        text
    }
}
