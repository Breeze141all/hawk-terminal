use crate::style::{self, Icon, icon_text};
use crate::widget::tooltip;
use data::chart::drawing::DrawingTool;
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, container, row};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Size, Theme, padding};

const ICON_SIZE: f32 = 22.0;

#[derive(Debug, Clone)]
pub enum ToolbarAction {
    SelectTool(DrawingTool),
    ClearDrawings,
    ToggleMagnet,
}

struct ToolIconProgram {
    tool: DrawingTool,
    is_active: bool,
}

impl<Message> canvas::Program<Message> for ToolIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();

        let stroke_color = if self.is_active {
            palette.primary.base.color
        } else {
            palette.background.base.text
        };

        let stroke = Stroke {
            style: canvas::Style::Solid(stroke_color),
            width: 1.4,
            line_cap: canvas::LineCap::Round,
            line_join: canvas::LineJoin::Round,
            ..Default::default()
        };

        let node_radius = 1.6;
        let empty_node = |frame: &mut Frame, pt: Point| {
            let bg_color = palette.background.base.color;
            frame.fill(&Path::circle(pt, node_radius), bg_color);
            frame.stroke(
                &Path::circle(pt, node_radius),
                Stroke {
                    style: canvas::Style::Solid(stroke_color),
                    width: 1.0,
                    ..Default::default()
                },
            );
        };

        match self.tool {
            DrawingTool::Rectangle => {
                // Square with 4 corner circle nodes
                let x1 = 4.0;
                let y1 = 4.0;
                let x2 = 18.0;
                let y2 = 18.0;

                frame.stroke(
                    &Path::rectangle(Point::new(x1, y1), Size::new(x2 - x1, y2 - y1)),
                    stroke,
                );
                empty_node(&mut frame, Point::new(x1, y1));
                empty_node(&mut frame, Point::new(x2, y1));
                empty_node(&mut frame, Point::new(x2, y2));
                empty_node(&mut frame, Point::new(x1, y2));
            }
            DrawingTool::ShortPosition => {
                // Two horizontal lines with circle anchor and "S" in between
                let x1 = 3.0;
                let x2 = 19.0;
                let y_top = 6.0;
                let y_bot = 16.0;

                frame.stroke(
                    &Path::line(Point::new(x1 + 2.0, y_top), Point::new(x2, y_top)),
                    stroke,
                );
                empty_node(&mut frame, Point::new(x1 + 2.0, y_top));

                frame.stroke(
                    &Path::line(Point::new(x1 + 2.0, y_bot), Point::new(x2, y_bot)),
                    stroke,
                );
                empty_node(&mut frame, Point::new(x1 + 2.0, y_bot));

                frame.fill_text(canvas::Text {
                    content: "S".to_string(),
                    position: Point::new(11.0, 11.0),
                    color: stroke_color,
                    size: iced::Pixels(9.0),
                    font: style::AZERET_MONO,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
            DrawingTool::Path => {
                // Connected zigzag line with nodes and arrowhead at end
                let p1 = Point::new(3.5, 15.0);
                let p2 = Point::new(8.5, 9.5);
                let p3 = Point::new(13.0, 14.0);
                let p4 = Point::new(18.5, 4.5);

                let path = Path::new(|builder| {
                    builder.move_to(p1);
                    builder.line_to(p2);
                    builder.line_to(p3);
                    builder.line_to(p4);
                });
                frame.stroke(&path, stroke);

                empty_node(&mut frame, p1);
                empty_node(&mut frame, p2);
                empty_node(&mut frame, p3);

                // Arrow head at p4
                let arrow = Path::new(|builder| {
                    builder.move_to(Point::new(p4.x - 4.5, p4.y));
                    builder.line_to(p4);
                    builder.line_to(Point::new(p4.x, p4.y + 4.5));
                });
                frame.stroke(&arrow, stroke);
            }
            DrawingTool::LongPosition => {
                // Two horizontal lines with circle anchor and "L" in between
                let x1 = 3.0;
                let x2 = 19.0;
                let y_top = 6.0;
                let y_bot = 16.0;

                frame.stroke(
                    &Path::line(Point::new(x1 + 2.0, y_top), Point::new(x2, y_top)),
                    stroke,
                );
                empty_node(&mut frame, Point::new(x1 + 2.0, y_top));

                frame.stroke(
                    &Path::line(Point::new(x1 + 2.0, y_bot), Point::new(x2, y_bot)),
                    stroke,
                );
                empty_node(&mut frame, Point::new(x1 + 2.0, y_bot));

                frame.fill_text(canvas::Text {
                    content: "L".to_string(),
                    position: Point::new(11.0, 11.0),
                    color: stroke_color,
                    size: iced::Pixels(9.0),
                    font: style::AZERET_MONO,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
            DrawingTool::Brush => {
                // Curved brush stroke icon
                let brush_path = Path::new(|builder| {
                    builder.move_to(Point::new(4.0, 16.0));
                    builder.quadratic_curve_to(Point::new(7.0, 11.0), Point::new(12.0, 14.0));
                    builder.quadratic_curve_to(Point::new(15.0, 16.0), Point::new(17.0, 9.0));
                    builder.line_to(Point::new(19.0, 6.0));
                });
                frame.stroke(&brush_path, stroke);

                // Brush bristle flare
                let tip = Path::new(|builder| {
                    builder.move_to(Point::new(4.0, 16.0));
                    builder.quadratic_curve_to(Point::new(3.0, 18.0), Point::new(6.0, 18.0));
                    builder.line_to(Point::new(8.0, 15.0));
                });
                frame.stroke(&tip, stroke);
            }
            DrawingTool::Trendline => {
                // Slanted segment with circle nodes at endpoints
                let p1 = Point::new(4.0, 17.0);
                let p2 = Point::new(18.0, 5.0);
                frame.stroke(&Path::line(p1, p2), stroke);
                empty_node(&mut frame, p1);
                empty_node(&mut frame, p2);
            }
            DrawingTool::HorizontalLine | DrawingTool::None => {}
        }

        vec![frame.into_geometry()]
    }
}

struct MagnetIconProgram {
    is_active: bool,
}

impl<Message> canvas::Program<Message> for MagnetIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();

        let stroke_color = if self.is_active {
            palette.primary.base.color
        } else {
            palette.background.base.text
        };

        let stroke = Stroke {
            style: canvas::Style::Solid(stroke_color),
            width: 2.0,
            line_cap: canvas::LineCap::Round,
            line_join: canvas::LineJoin::Round,
            ..Default::default()
        };

        // U-shaped magnet path
        let u_path = Path::new(|builder| {
            builder.move_to(Point::new(6.0, 5.0));
            builder.line_to(Point::new(6.0, 12.0));
            builder.arc_to(Point::new(6.0, 17.0), Point::new(11.0, 17.0), 5.0);
            builder.arc_to(Point::new(16.0, 17.0), Point::new(16.0, 12.0), 5.0);
            builder.line_to(Point::new(16.0, 5.0));
        });
        frame.stroke(&u_path, stroke);

        // Pole tip markers
        let cap_stroke = Stroke {
            style: canvas::Style::Solid(if self.is_active {
                palette.primary.strong.color
            } else {
                palette.background.base.text.scale_alpha(0.6)
            }),
            width: 2.0,
            line_cap: canvas::LineCap::Butt,
            ..Default::default()
        };
        frame.stroke(
            &Path::line(Point::new(5.0, 8.0), Point::new(7.0, 8.0)),
            cap_stroke,
        );
        frame.stroke(
            &Path::line(Point::new(15.0, 8.0), Point::new(17.0, 8.0)),
            cap_stroke,
        );

        vec![frame.into_geometry()]
    }
}

pub fn view<'a, Message: 'a + Clone>(
    active_tool: DrawingTool,
    has_drawings: bool,
    magnet_mode: bool,
    on_action: impl Fn(ToolbarAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    let tools = [
        (DrawingTool::Rectangle, "Rectangle"),
        (DrawingTool::ShortPosition, "Short Position"),
        (DrawingTool::Path, "Path / Polyline"),
        (DrawingTool::LongPosition, "Long Position"),
        (DrawingTool::Brush, "Brush"),
        (DrawingTool::Trendline, "Trendline"),
    ];

    let drag_handle = container(icon_text(Icon::DragHandle, 13).style(|theme: &Theme| {
        let palette = theme.extended_palette();
        iced::widget::text::Style {
            color: Some(palette.background.base.text.scale_alpha(0.4)),
        }
    }))
    .padding(padding::left(4).right(4))
    .align_y(Alignment::Center);

    let mut row_items = row![drag_handle].spacing(2).align_y(Alignment::Center);

    for (tool, label) in tools {
        let is_active = active_tool == tool;
        let next_tool = if is_active { DrawingTool::None } else { tool };

        let icon = Canvas::new(ToolIconProgram { tool, is_active })
            .width(Length::Fixed(ICON_SIZE))
            .height(Length::Fixed(ICON_SIZE));

        let btn = button(icon)
            .padding(4)
            .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_active))
            .on_press(on_action(ToolbarAction::SelectTool(next_tool)));

        let tip_btn = tooltip(btn, Some(label), iced::widget::tooltip::Position::Top);

        row_items = row_items.push(tip_btn);
    }

    let magnet_icon = Canvas::new(MagnetIconProgram {
        is_active: magnet_mode,
    })
    .width(Length::Fixed(ICON_SIZE))
    .height(Length::Fixed(ICON_SIZE));

    let magnet_btn = button(magnet_icon)
        .padding(4)
        .style(move |theme: &Theme, status| toolbar_button_style(theme, status, magnet_mode))
        .on_press(on_action(ToolbarAction::ToggleMagnet));

    let tip_magnet = tooltip(
        magnet_btn,
        Some(if magnet_mode {
            "Smart Magnet: ON (Ctrl to invert)"
        } else {
            "Smart Magnet: OFF (Ctrl to invert)"
        }),
        iced::widget::tooltip::Position::Top,
    );

    row_items = row_items.push(tip_magnet);

    if has_drawings {
        let trash_icon = icon_text(Icon::TrashBin, 13);
        let clear_btn = button(trash_icon)
            .padding(4)
            .style(|theme: &Theme, status| toolbar_clear_button_style(theme, status))
            .on_press(on_action(ToolbarAction::ClearDrawings));

        let tip_clear = tooltip(
            clear_btn,
            Some("Clear All Drawings"),
            iced::widget::tooltip::Position::Top,
        );

        row_items = row_items.push(tip_clear);
    }

    container(row_items)
        .padding(padding::top(3).bottom(3).left(4).right(4))
        .style(toolbar_container_style)
        .into()
}

pub fn toolbar_button_style(
    theme: &Theme,
    status: button::Status,
    is_active: bool,
) -> button::Style {
    let palette = theme.extended_palette();

    if is_active {
        let alpha = if palette.is_dark { 0.25 } else { 0.2 };
        return button::Style {
            background: Some(palette.primary.base.color.scale_alpha(alpha).into()),
            text_color: palette.primary.base.color,
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: palette.primary.base.color,
            },
            ..Default::default()
        };
    }

    match status {
        button::Status::Hovered => button::Style {
            background: Some(
                palette
                    .background
                    .weak
                    .color
                    .scale_alpha(if palette.is_dark { 0.6 } else { 0.4 })
                    .into(),
            ),
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(
                palette
                    .background
                    .strong
                    .color
                    .scale_alpha(if palette.is_dark { 0.7 } else { 0.5 })
                    .into(),
            ),
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Active | button::Status::Disabled => button::Style {
            background: None,
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

pub fn toolbar_clear_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    match status {
        button::Status::Hovered => button::Style {
            background: Some(
                palette
                    .danger
                    .base
                    .color
                    .scale_alpha(if palette.is_dark { 0.22 } else { 0.16 })
                    .into(),
            ),
            text_color: palette.danger.strong.color,
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: palette.danger.base.color.scale_alpha(0.5),
            },
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(
                palette
                    .danger
                    .base
                    .color
                    .scale_alpha(if palette.is_dark { 0.32 } else { 0.26 })
                    .into(),
            ),
            text_color: palette.danger.strong.color,
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: palette.danger.base.color,
            },
            ..Default::default()
        },
        button::Status::Active | button::Status::Disabled => button::Style {
            background: None,
            text_color: palette.danger.base.color,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

pub fn toolbar_container_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(
            iced::Color {
                a: if palette.is_dark { 0.94 } else { 0.97 },
                ..palette.background.base.color
            }
            .into(),
        ),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.background.weak.color,
        },
        shadow: iced::Shadow {
            color: iced::Color::BLACK.scale_alpha(if palette.is_dark { 0.45 } else { 0.15 }),
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        text_color: Some(palette.background.base.text),
        ..Default::default()
    }
}
