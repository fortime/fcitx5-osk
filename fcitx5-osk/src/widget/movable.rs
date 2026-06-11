use std::{
    mem,
    ops::{Deref, DerefMut},
    rc::Rc,
    time::{Duration, Instant},
};

use iced::{
    Alignment, Element, Event, Length, Padding, Pixels, Point, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout,
        overlay::{self, Group},
        renderer,
        text::Renderer as TextRenderer,
        widget::{Operation, Tree, tree},
    },
    alignment::Vertical,
    border,
    mouse::{
        Button as MouseButton, Cursor as MouseCursor, Event as MouseEvent,
        Interaction as MouseInteraction,
    },
    touch::{Event as TouchEvent, Finger as TouchFinger},
    widget::{
        Container, Row, Space, Text,
        button::{
            Catalog as ButtonCatalog, Status as ButtonStatus, Style as ButtonStyle,
            StyleFn as ButtonStyleFn,
        },
        container::{
            Catalog as ContainerCatalog, Style as ContainerStyle, StyleFn as ContainerStyleFn,
        },
        text::Catalog as TextCatalog,
    },
};
use iced_drop::widget::droppable::Droppable;

use crate::{
    misc::LocalShell,
    widget::{
        ExtButton, ExtButtonCatalog,
        button::{self},
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
        tree.diff_children(std::slice::from_ref(&self.content));
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

#[derive(Default)]
enum MovableListSlot<'a, Id, Message, Theme, Renderer> {
    // before sealed
    #[default]
    EmptyRaw,
    OccupiedRaw(Id, Element<'a, Message, Theme, Renderer>),
    // it is used to cache empty space element
    Empty(
        MovableListEmptySlotLayout,
        Element<'a, MovableListInnerMessage<Message>, Theme, Renderer>,
    ),
    Occupied(
        Id,
        Element<'a, MovableListInnerMessage<Message>, Theme, Renderer>,
    ),
}

impl<Id, Message, Theme, Renderer> MovableListSlot<'_, Id, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tree(&self) -> Tree {
        match self {
            MovableListSlot::EmptyRaw => unreachable!("it shouldn't exist after sealed"),
            MovableListSlot::OccupiedRaw(_, _) => unreachable!("it shouldn't exist after sealed"),
            MovableListSlot::Empty(_, i) => Tree::new(i),
            MovableListSlot::Occupied(_, i) => Tree::new(i),
        }
    }

    fn diff_by(&self, tree: &mut Tree) {
        match self {
            MovableListSlot::EmptyRaw => unreachable!("it shouldn't exist after sealed"),
            MovableListSlot::OccupiedRaw(_, _) => unreachable!("it shouldn't exist after sealed"),
            MovableListSlot::Empty(_, i) => tree.diff(i),
            MovableListSlot::Occupied(_, i) => tree.diff(i),
        }
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

        (Box::new(move |theme: &Self, status| f(theme, status)) as ButtonStyleFn<'a, Self>).into()
    }

    fn item_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a> {
        let f = Rc::clone(&class.item_style_fn);

        (Box::new(move |theme: &Self| f(theme)) as ContainerStyleFn<'a, Self>).into()
    }

    fn dropping_empty_slot_class<'a>(
        class: &<Self as MovableListCatalog>::Class<'a>,
    ) -> <Self as ContainerCatalog>::Class<'a> {
        let f = Rc::clone(&class.dropping_empty_slot_style_fn);

        (Box::new(move |theme: &Self| f(theme)) as ContainerStyleFn<'a, Self>).into()
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

struct MovableListElementsGaurd<'a, 'b, Id, Message, Theme, Renderer>
where
    Theme: MovableListCatalog,
{
    list: &'a mut MovableList<'b, Id, Message, Theme, Renderer>,
    id_or_layouts: Vec<Result<Id, MovableListEmptySlotLayout>>,
    elements: Vec<Element<'b, MovableListInnerMessage<Message>, Theme, Renderer>>,
}

impl<'b, Id, Message, Theme, Renderer> Drop
    for MovableListElementsGaurd<'_, 'b, Id, Message, Theme, Renderer>
where
    Theme: MovableListCatalog,
{
    fn drop(&mut self) {
        assert_eq!(
            self.id_or_layouts.len(),
            self.elements.len(),
            "the length of `id_or_layouts` and the length of elements are not equal"
        );
        self.list.items = self
            .id_or_layouts
            .drain(..)
            .zip(self.elements.drain(..))
            .map(|(id_or_layout, element)| match id_or_layout {
                Ok(id) => MovableListSlot::Occupied(id, element),
                Err(layout) => MovableListSlot::Empty(layout, element),
            })
            .collect();
    }
}

impl<'b, Id, Message, Theme, Renderer> Deref
    for MovableListElementsGaurd<'_, 'b, Id, Message, Theme, Renderer>
where
    Theme: MovableListCatalog,
{
    type Target = [Element<'b, MovableListInnerMessage<Message>, Theme, Renderer>];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl<'b, Id, Message, Theme, Renderer> DerefMut
    for MovableListElementsGaurd<'_, 'b, Id, Message, Theme, Renderer>
where
    Theme: MovableListCatalog,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.elements
    }
}

/// A container of a list that its items can be dragged to reorder.
pub struct MovableList<'a, Id, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: MovableListCatalog,
{
    items: Vec<MovableListSlot<'a, Id, Message, Theme, Renderer>>,
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
            items: vec![MovableListSlot::EmptyRaw],
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
        self.items
            .push(MovableListSlot::OccupiedRaw(id, item.into()));
        self.items.push(MovableListSlot::EmptyRaw);
        self
    }

    fn seal(&mut self) {
        for idx in 0..self.items.len() {
            let mut slot = mem::take(&mut self.items[idx]);
            match slot {
                MovableListSlot::EmptyRaw => {
                    let layout = Self::empty_slot_layout(idx, self.items.len(), None, None);
                    slot = MovableListSlot::Empty(layout, self.empty_slot_element(layout))
                }
                MovableListSlot::OccupiedRaw(id, element) => {
                    let mut element = element.map(MovableListInnerMessage::OuterMessage);
                    element = if self.on_remove.is_some() {
                        let mut delete_text = Text::new("x").center();
                        if let Some(text_size) = self.remove_button_text_size {
                            delete_text = delete_text.size(text_size);
                        }
                        let mut remove_button = ExtButton::new(delete_text)
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
                    let mut container =
                        Container::new(element).class(Theme::item_class(&self.class));
                    if let Some(padding) = self.item_padding {
                        container = container.padding(padding);
                    }
                    element = if self.on_drop.is_some() {
                        Droppable::new(container)
                            .drag_center(true)
                            .drag_size(Size::new(0., 0.))
                            .drag_hide(true)
                            .on_drag({
                                move |point, _rectangle| MovableListInnerMessage::Drag(point, idx)
                            })
                            .on_drop(|point, _rectangle| MovableListInnerMessage::Drop(point))
                            .on_cancel(MovableListInnerMessage::Cancel)
                            .into()
                    } else {
                        container.into()
                    };
                    slot = MovableListSlot::Occupied(id, element)
                }
                _ => {}
            }
            self.items[idx] = slot;
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
                        layout.before = 0;
                    } else if dragged_idx == 1 && dragged_idx + 1 == dropping_idx {
                        layout.before = 0;
                    } else if idx + 1 == len {
                        layout.after = 0;
                    } else if dropping_idx + 1 == dragged_idx && dragged_idx + 2 == len {
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
        &self,
        layout: MovableListEmptySlotLayout,
    ) -> Element<'a, MovableListInnerMessage<Message>, Theme, Renderer> {
        let mut padding = Padding::ZERO;
        if self.horizontal {
            padding.left = self.spacing / 2. * layout.before as f32;
            padding.right = self.spacing / 2. * layout.after as f32;
        } else {
            padding.top = self.spacing / 2. * layout.before as f32;
            padding.bottom = self.spacing / 2. * layout.after as f32;
        }
        let middle = if let Some(middle) = layout.middle {
            Container::new(Space::new().width(middle.width).height(middle.height))
                .class(Theme::dropping_empty_slot_class(&self.class))
        } else {
            Container::new(Space::new())
        };
        Container::new(middle).padding(padding).into()
    }

    fn as_layout_elements<'b>(
        &'b mut self,
        state: &'b MovableListState,
    ) -> MovableListElementsGaurd<'b, 'a, Id, Message, Theme, Renderer> {
        let mut id_or_layouts = Vec::with_capacity(self.items.len());
        let mut elements = Vec::with_capacity(self.items.len());
        let items: Vec<_> = self.items.drain(..).enumerate().collect();
        let len = items.len();
        for (idx, item) in items {
            match item {
                MovableListSlot::EmptyRaw => {
                    unreachable!("`as_layout_elements` shouldn't be called before `seal`")
                }
                MovableListSlot::OccupiedRaw(_, _) => {
                    unreachable!("`as_layout_elements` shouldn't be called before `seal`")
                }
                MovableListSlot::Empty(layout, element) => {
                    let new_layout =
                        Self::empty_slot_layout(idx, len, state.dragged, state.dropping);
                    if new_layout != layout {
                        tracing::debug!(
                            "New empty slot layout at {idx}, from [{layout:?}] to [{new_layout:?}]"
                        );
                        id_or_layouts.push(Err(new_layout));
                        elements.push(self.empty_slot_element(new_layout));
                    } else {
                        id_or_layouts.push(Err(layout));
                        elements.push(element);
                    }
                }
                MovableListSlot::Occupied(id, element) => {
                    id_or_layouts.push(Ok(id));
                    elements.push(element);
                }
            }
        }
        MovableListElementsGaurd {
            list: self,
            id_or_layouts,
            elements,
        }
    }

    fn is_dropping(
        &self,
        layout: Layout<'_>,
        point: Point,
        dragged_slot_bounds: Rectangle,
    ) -> Option<usize> {
        let bounds = layout.bounds();
        if self.horizontal {
            if point.y < bounds.y || point.y > bounds.y + bounds.height {
                return None;
            }
            let mut max_idx = 0;
            let x_bound = point.x - dragged_slot_bounds.width / 2.;
            for idx in 1..self.items.len() {
                if let MovableListSlot::Occupied(_, _) = &self.items[idx] {
                    let slot_bounds = layout.child(idx).bounds();
                    if x_bound < slot_bounds.x {
                        return Some(idx - 1);
                    }
                    max_idx = max_idx.max(idx + 1);
                }
            }
            Some(max_idx)
        } else {
            if point.x < bounds.x || point.x > bounds.x + bounds.width {
                return None;
            }
            let mut max_idx = 0;
            let y_bound = point.y - dragged_slot_bounds.height / 2.;
            for idx in 1..self.items.len() {
                if let MovableListSlot::Occupied(_, _) = &self.items[idx] {
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
        self.items.iter().map(MovableListSlot::tree).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        if tree.children.len() > self.items.len() {
            tree.children.truncate(self.items.len());
        }

        for (slot_state, slot) in tree.children.iter_mut().zip(self.items.iter()) {
            slot.diff_by(slot_state);
        }

        if tree.children.len() < self.items.len() {
            tree.children.extend(
                self.items[tree.children.len()..]
                    .iter()
                    .map(MovableListSlot::tree),
            );
        }
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state: &MovableListState = tree.state.downcast_ref();
        let axis = if self.horizontal {
            layout::flex::Axis::Horizontal
        } else {
            layout::flex::Axis::Vertical
        };
        layout::flex::resolve(
            axis,
            renderer,
            limits,
            Length::Fill,
            Length::Fill,
            Padding::ZERO,
            // spacing will be representing by empty slot
            0.,
            Alignment::Start,
            &mut self.as_layout_elements(state),
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
            self.items
                .iter_mut()
                .enumerate()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|(((idx, slot), slot_state), slot_layout)| {
                    if !state.is_dragged(idx)
                        && let MovableListSlot::Occupied(_, element) = slot
                    {
                        // only operates on non-dragged item
                        element.as_widget_mut().operate(
                            slot_state,
                            slot_layout,
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
        for (((idx, slot), slot_state), slot_layout) in self
            .items
            .iter_mut()
            .enumerate()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            if let MovableListSlot::Occupied(_, element) = slot {
                let is_captured = local_shell.is_event_captured();
                element.as_widget_mut().update(
                    slot_state,
                    event,
                    slot_layout,
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
        for local_message in local_messages {
            match local_message {
                MovableListInnerMessage::Drag(_point, idx) => {
                    // NOTE the `point` is not correct inside a scrollable, use the position from
                    // `cursor` instead
                    let Some(point) = cursor.position() else {
                        tracing::warn!(
                            "The position of cursor isn't available when there is a drag event"
                        );
                        continue;
                    };
                    let dragged_bounds = if let Some((dragged_idx, dragged_bounds)) = state.dragged
                    {
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
                        let MovableListSlot::Occupied(id, _) = &self.items[idx] else {
                            unreachable!("the slot[{idx}] isn't a MovableListSlot::Occupied");
                        };
                        shell.publish(on_drag(id, point));
                    }
                    let new_dropping = self.is_dropping(layout, point, dragged_bounds);
                    if state.dropping != new_dropping {
                        state.dropping = new_dropping;
                        shell.invalidate_layout();
                    }
                }
                MovableListInnerMessage::Drop(_point) => {
                    // NOTE the `point` is not correct inside a scrollable, use the position from
                    // `cursor` instead
                    let Some(point) = cursor.position() else {
                        tracing::warn!(
                            "The position of cursor isn't available when there is a drag event"
                        );
                        continue;
                    };
                    if let Some((dragged_idx, dragged_bounds)) = state.dragged {
                        state.dropping = self.is_dropping(layout, point, dragged_bounds);
                        if let Some(on_drop) = &self.on_drop
                            && let Some(dropping_idx) = state.dropping
                        {
                            shell.publish(on_drop(
                                &self
                                    .items
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(idx, slot)| {
                                        if idx == dropping_idx {
                                            let MovableListSlot::Occupied(id, _) = &self.items[dragged_idx] else {
                                                unreachable!("the slot[{dragged_idx}] isn't a MovableListSlot::Occupied");
                                            };
                                            Some(id)
                                        } else if idx == dragged_idx {
                                            None
                                        } else if let MovableListSlot::Occupied(id, _) = slot {
                                            Some(id)
                                        } else {
                                            None
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
                        shell.publish(on_drop(
                            &self
                                .items
                                .iter()
                                .filter_map(|slot| {
                                    if let MovableListSlot::Occupied(id, _) = slot {
                                        Some(id)
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>(),
                        ));
                    }
                    if state.dragged.take().is_some() {
                        shell.invalidate_layout();
                    }
                    state.dropping.take();
                }
                MovableListInnerMessage::Remove(idx) => {
                    tracing::debug!("Remove slot[{idx}]");
                    if let Some(on_remove) = &self.on_remove {
                        if let MovableListSlot::Occupied(id, _) = &self.items[idx] {
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
            self.items
                .iter()
                .enumerate()
                .zip(&tree.children)
                .zip(layout.children())
                .map(|(((idx, slot), slot_state), slot_layout)| {
                    if !state.is_dragged(idx)
                        && let MovableListSlot::Occupied(_, element) = slot
                    {
                        // only operates on non-dragged item
                        return element.as_widget().mouse_interaction(
                            slot_state,
                            slot_layout,
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

        for (((idx, slot), slot_state), slot_layout) in self
            .items
            .iter()
            .enumerate()
            .zip(&tree.children)
            .zip(layout.children())
            .filter(|(_, layout)| layout.bounds().intersects(viewport))
        {
            if !state.is_dragged(idx) {
                match slot {
                    MovableListSlot::Empty(_, element) => element.as_widget().draw(
                        slot_state,
                        renderer,
                        theme,
                        renderer_style,
                        slot_layout,
                        cursor,
                        viewport,
                    ),
                    MovableListSlot::Occupied(_, element) => element.as_widget().draw(
                        slot_state,
                        renderer,
                        theme,
                        renderer_style,
                        slot_layout,
                        cursor,
                        viewport,
                    ),
                    _ => {}
                }
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
        let children = self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((slot, slot_state), slot_layout)| match slot {
                MovableListSlot::EmptyRaw => unreachable!("it shouldn't exist after sealed"),
                MovableListSlot::OccupiedRaw(_, _) => {
                    unreachable!("it shouldn't exist after sealed")
                }
                MovableListSlot::Empty(_, _) => None,
                MovableListSlot::Occupied(_, element) => element.as_widget_mut().overlay(
                    slot_state,
                    slot_layout,
                    renderer,
                    viewport,
                    translation,
                ),
            })
            .collect::<Vec<_>>();

        (!children.is_empty())
            .then(|| Group::with_children(children).overlay())
            .map(|o| {
                o.map(&|_| {
                    unreachable!(
                        "the implementation of iced_drop has changed, it generates message now"
                    )
                })
            })
    }
}

impl<'a, Id, Message, Theme, Renderer> From<MovableList<'a, Id, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Id: 'a,
    Message: 'a + Clone,
    Theme: 'a + MovableListCatalog,
    <Theme as ButtonCatalog>::Class<'a>: From<ButtonStyleFn<'a, Theme>>,
    Renderer: 'a + renderer::Renderer + TextRenderer,
{
    fn from(mut widget: MovableList<'a, Id, Message, Theme, Renderer>) -> Self {
        widget.seal();
        Element::new(widget)
    }
}
