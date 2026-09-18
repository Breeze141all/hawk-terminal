use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::event::Event;
use iced::mouse;
use iced::{Element, Length, Point, Rectangle, Size, Vector};

#[derive(Debug, Clone, Default)]
pub struct DragState {
    pub is_dragging: bool,
    pub drag_offset: Vector,
}

pub struct DraggableOverlay<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    position: Point,
    drag_handle_width: Option<f32>,
    on_drag: Box<dyn Fn(Point) -> Message + 'a>,
}

impl<'a, Message, Theme, Renderer> DraggableOverlay<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new<F>(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        position: Point,
        on_drag: F,
    ) -> Self
    where
        F: Fn(Point) -> Message + 'a,
    {
        Self {
            content: content.into(),
            position,
            drag_handle_width: None,
            on_drag: Box::new(on_drag),
        }
    }

    pub fn drag_handle_width(mut self, width: f32) -> Self {
        self.drag_handle_width = Some(width);
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for DraggableOverlay<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max_size = limits.max();
        let content_limits = layout::Limits::new(Size::ZERO, max_size);
        let content_node =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &content_limits);

        let content_size = content_node.size();
        let max_x = (max_size.width - content_size.width).max(0.0);
        let max_y = (max_size.height - content_size.height).max(0.0);

        let clamped_x = self.position.x.clamp(0.0, max_x);
        let clamped_y = self.position.y.clamp(0.0, max_y);

        let content_node = content_node.move_to(Point::new(clamped_x, clamped_y));

        layout::Node::with_children(max_size, vec![content_node])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DragState>();
        let mut content_layout = layout.children();
        let content_node = content_layout.next();

        if state.is_dragging {
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(cursor_pos) = cursor.position_in(layout.bounds()) {
                        let new_x = cursor_pos.x - state.drag_offset.x;
                        let new_y = cursor_pos.y - state.drag_offset.y;

                        let bounds = layout.bounds();
                        let content_bounds =
                            content_node.map_or(Rectangle::default(), |c| c.bounds());
                        let max_x = (bounds.width - content_bounds.width).max(0.0);
                        let max_y = (bounds.height - content_bounds.height).max(0.0);

                        let clamped_pos =
                            Point::new(new_x.clamp(0.0, max_x), new_y.clamp(0.0, max_y));
                        shell.publish((self.on_drag)(clamped_pos));
                        shell.capture_event();
                        return;
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    state.is_dragging = false;
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }

        // First let child handle interaction (buttons, clicks)
        if let Some(c_node) = content_node {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                c_node,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        // If child didn't capture and left button was pressed on content / drag handle
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(c_node) = content_node
        {
            let content_bounds = c_node.bounds();
            if let Some(cursor_pos) = cursor.position() {
                let can_drag = if let Some(handle_w) = self.drag_handle_width {
                    let handle_bounds = Rectangle {
                        width: handle_w,
                        ..content_bounds
                    };
                    handle_bounds.contains(cursor_pos)
                } else {
                    content_bounds.contains(cursor_pos)
                };

                if can_drag {
                    state.is_dragging = true;
                    state.drag_offset = Vector::new(
                        cursor_pos.x - content_bounds.x,
                        cursor_pos.y - content_bounds.y,
                    );
                    shell.capture_event();
                }
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(content_layout) = layout.children().next() {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                content_layout,
                cursor,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<DragState>();
        if state.is_dragging {
            return mouse::Interaction::Grabbing;
        }

        if let Some(content_layout) = layout.children().next() {
            if let Some(cursor_pos) = cursor.position()
                && let Some(handle_w) = self.drag_handle_width
            {
                let handle_bounds = Rectangle {
                    width: handle_w,
                    ..content_layout.bounds()
                };
                if handle_bounds.contains(cursor_pos) {
                    return mouse::Interaction::Grab;
                }
            }

            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                content_layout,
                cursor,
                viewport,
                renderer,
            )
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<DraggableOverlay<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(widget: DraggableOverlay<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
