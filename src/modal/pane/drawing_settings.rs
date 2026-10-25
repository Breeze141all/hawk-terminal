use crate::screen::dashboard::pane::{Event, Message};
use crate::style::{self, Icon, icon_text};
use data::chart::drawing::{
    Drawing, DrawingKind, LineStyle, TextAlignH, TextAlignV, color_to_hex, get_recent_colors,
};
use iced::widget::{
    button, checkbox, column, container, pane_grid, row, rule, scrollable, slider, space, text,
    text_input,
};
use iced::{Alignment, Border, Color, Element, Length, Theme, border};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingSettingsTab {
    #[default]
    Style,
    Text,
    Coordinates,
    Visibility,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawingSettingsState {
    pub drawing: Drawing,
    pub active_tab: DrawingSettingsTab,
    pub hex_input: String,
    pub hex_valid: bool,
    pub text_input: String,
    pub text_hex_input: String,
    pub coord_p1: String,
    pub coord_p2: String,
    pub coord_t1: String,
    pub coord_t2: String,
}

impl DrawingSettingsState {
    pub fn new(drawing: &Drawing) -> Self {
        let (p1, p2, t1, t2) = match &drawing.kind {
            DrawingKind::HorizontalLine { price } => (
                price.to_string(),
                String::new(),
                String::new(),
                String::new(),
            ),
            DrawingKind::Trendline { p1, p2 } => (
                p1.1.to_string(),
                p2.1.to_string(),
                p1.0.to_string(),
                p2.0.to_string(),
            ),
            DrawingKind::Rectangle { p1, p2 } => (
                p1.1.to_string(),
                p2.1.to_string(),
                p1.0.to_string(),
                p2.0.to_string(),
            ),
            DrawingKind::Position {
                entry,
                stop_price,
                target_price,
                end_time,
                ..
            } => (
                entry.1.to_string(),
                stop_price.to_string(),
                entry.0.to_string(),
                end_time
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| target_price.to_string()),
            ),
            _ => (String::new(), String::new(), String::new(), String::new()),
        };

        let hex = color_to_hex(drawing.color);
        let text_str = drawing
            .annotation
            .as_ref()
            .map(|a| a.text.clone())
            .unwrap_or_default();
        let text_hex = drawing
            .annotation
            .as_ref()
            .map(|a| color_to_hex(a.color))
            .unwrap_or_else(|| "#FFFFFF".to_string());

        Self {
            drawing: drawing.clone(),
            active_tab: DrawingSettingsTab::Style,
            hex_input: hex,
            hex_valid: true,
            text_input: text_str,
            text_hex_input: text_hex,
            coord_p1: p1,
            coord_p2: p2,
            coord_t1: t1,
            coord_t2: t2,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DrawingSettingsAction {
    SelectTab(DrawingSettingsTab),
    HexChanged(String),
    ApplyHex,
    RgbChanged(f32, f32, f32),
    SelectRecentColor([f32; 4]),
    SetLineStyle(LineStyle),
    SetWidth(f32),
    SetOpacity(f32),
    ToggleExtendLeft,
    ToggleExtendRight,
    ToggleArrowLeft,
    ToggleArrowRight,
    SetFillOpacity(f32),
    // Text Tab
    TextChanged(String),
    TextHexChanged(String),
    ApplyTextHex,
    SetFontSize(f32),
    SetTextAlignH(TextAlignH),
    SetTextAlignV(TextAlignV),
    // Coordinates Tab
    CoordP1Changed(String),
    CoordP2Changed(String),
    CoordT1Changed(String),
    CoordT2Changed(String),
    // Visibility Tab
    ToggleVisibilitySeconds,
    ToggleVisibilityMinutes,
    ToggleVisibilityHours,
    ToggleVisibilityDays,
    ToggleVisibilityWeeks,
    Close,
}

pub fn drawing_settings_view<'a>(
    pane: pane_grid::Pane,
    state: &'a DrawingSettingsState,
) -> Element<'a, Message> {
    let tool_name = match &state.drawing.kind {
        DrawingKind::HorizontalLine { .. } => "Horizontal Line",
        DrawingKind::Trendline { .. } => "Trendline",
        DrawingKind::Rectangle { .. } => "Rectangle",
        DrawingKind::Brush { .. } => "Brush",
        DrawingKind::Path { .. } => "Path",
        DrawingKind::Position { is_long: true, .. } => "Long Position",
        DrawingKind::Position { is_long: false, .. } => "Short Position",
    };

    let title_bar = row![
        text(format!("{tool_name} Properties"))
            .size(14)
            .font(style::AZERET_MONO),
        space(),
        button(icon_text(Icon::Close, 11))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(Message::PaneEvent(pane, Event::HideModal))
            .padding([2, 4]),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // Tab buttons
    let tab_btn = |tab: DrawingSettingsTab, label: &'static str| {
        let is_active = state.active_tab == tab;
        button(
            text(label)
                .size(11)
                .font(style::AZERET_MONO)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .padding([4, 10])
        .style(move |theme: &Theme, status| {
            let p = theme.extended_palette();
            if is_active {
                button::Style {
                    background: Some(p.primary.weak.color.into()),
                    text_color: p.primary.weak.text,
                    border: Border {
                        radius: 3.0.into(),
                        width: 1.0,
                        color: p.primary.strong.color,
                    },
                    ..Default::default()
                }
            } else {
                style::button::transparent(theme, status, false)
            }
        })
        .on_press(Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::SelectTab(tab)),
        ))
    };

    let tab_bar = row![
        tab_btn(DrawingSettingsTab::Style, "Style"),
        tab_btn(DrawingSettingsTab::Text, "Text"),
        tab_btn(DrawingSettingsTab::Coordinates, "Coordinates"),
        tab_btn(DrawingSettingsTab::Visibility, "Visibility"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let tab_content: Element<'a, Message> = match state.active_tab {
        DrawingSettingsTab::Style => view_style_tab(pane, state),
        DrawingSettingsTab::Text => view_text_tab(pane, state),
        DrawingSettingsTab::Coordinates => view_coordinates_tab(pane, state),
        DrawingSettingsTab::Visibility => view_visibility_tab(pane, state),
    };

    let content = column![
        title_bar,
        tab_bar,
        rule::horizontal(1),
        scrollable(tab_content).height(Length::Fixed(280.0)),
    ]
    .spacing(10)
    .width(Length::Fixed(360.0));

    container(content)
        .padding(14)
        .style(style::chart_modal)
        .into()
}

fn view_style_tab<'a>(
    pane: pane_grid::Pane,
    state: &'a DrawingSettingsState,
) -> Element<'a, Message> {
    let d = &state.drawing;
    let current_color = d.color;
    let r_byte = (current_color[0] * 255.0).round() as u8;
    let g_byte = (current_color[1] * 255.0).round() as u8;
    let b_byte = (current_color[2] * 255.0).round() as u8;
    let alpha_pct = (current_color[3] * 100.0).round() as u8;

    // 1. Line Style buttons
    let style_btn = |ls: LineStyle, label: &'static str| {
        let is_active = d.line_style == ls;
        button(text(label).size(10).font(style::AZERET_MONO))
            .padding([3, 8])
            .style(move |theme, status| style::button::modifier(theme, status, is_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetLineStyle(ls)),
            ))
    };

    let line_style_row = row![
        container(text("Line:").size(11).font(style::AZERET_MONO)).width(55),
        style_btn(LineStyle::Solid, "Solid"),
        style_btn(LineStyle::Dashed, "Dashed"),
        style_btn(LineStyle::Dotted, "Dotted"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // 2. Line Width buttons
    let width_btn = |w: f32, label: &'static str| {
        let is_active = (d.width - w).abs() < 0.4;
        button(text(label).size(10).font(style::AZERET_MONO))
            .padding([3, 7])
            .style(move |theme, status| style::button::modifier(theme, status, is_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetWidth(w)),
            ))
    };

    let width_row = row![
        container(text("Width:").size(11).font(style::AZERET_MONO)).width(55),
        width_btn(1.0, "1px"),
        width_btn(2.0, "2px"),
        width_btn(3.0, "3px"),
        width_btn(4.0, "4px"),
        width_btn(6.0, "6px"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // 3. Opacity slider
    let opacity_slider = slider(10..=100, alpha_pct, move |new_pct| {
        Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::SetOpacity(new_pct as f32 / 100.0)),
        )
    })
    .step(5u8)
    .width(Length::Fixed(180.0));

    let opacity_row = row![
        container(text("Opacity:").size(11).font(style::AZERET_MONO)).width(55),
        opacity_slider,
        text(format!("{}%", alpha_pct))
            .size(10)
            .font(style::AZERET_MONO),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 4. Color HEX Input with Swatch Preview
    let swatch_preview = container("")
        .width(18)
        .height(18)
        .style(move |_| container::Style {
            background: Some(
                Color::from_rgba(
                    current_color[0],
                    current_color[1],
                    current_color[2],
                    current_color[3],
                )
                .into(),
            ),
            border: border::rounded(3).width(1).color(Color::WHITE),
            ..Default::default()
        });

    let hex_field = text_input("#RRGGBB", &state.hex_input)
        .on_input(move |val| {
            Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::HexChanged(val)),
            )
        })
        .on_submit(Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::ApplyHex),
        ))
        .width(Length::Fixed(90.0))
        .padding(4)
        .style(|theme, status| style::validated_text_input(theme, status, true));

    let r_slider = slider(0..=255, r_byte, move |v| {
        Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::RgbChanged(
                v as f32 / 255.0,
                current_color[1],
                current_color[2],
            )),
        )
    })
    .width(Length::Fixed(45.0));

    let g_slider = slider(0..=255, g_byte, move |v| {
        Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::RgbChanged(
                current_color[0],
                v as f32 / 255.0,
                current_color[2],
            )),
        )
    })
    .width(Length::Fixed(45.0));

    let b_slider = slider(0..=255, b_byte, move |v| {
        Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::RgbChanged(
                current_color[0],
                current_color[1],
                v as f32 / 255.0,
            )),
        )
    })
    .width(Length::Fixed(45.0));

    let hex_row = row![
        container(text("Color:").size(11).font(style::AZERET_MONO)).width(55),
        swatch_preview,
        hex_field,
        button(text("Apply").size(10).font(style::AZERET_MONO))
            .padding([3, 6])
            .style(|theme, status| style::button::confirm(theme, status, false))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::ApplyHex),
            )),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let rgb_row = row![
        container(text("RGB:").size(11).font(style::AZERET_MONO)).width(55),
        text("R").size(9).font(style::AZERET_MONO),
        r_slider,
        text("G").size(9).font(style::AZERET_MONO),
        g_slider,
        text("B").size(9).font(style::AZERET_MONO),
        b_slider,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // 5. Recent Color Swatches (Palette)
    let recent_colors = get_recent_colors();
    let mut swatches_row = row![].spacing(4).align_y(Alignment::Center);
    for rc in recent_colors.into_iter().take(10) {
        let rc_color = Color::from_rgba(rc[0], rc[1], rc[2], rc[3]);
        let is_selected_rc = (rc[0] - current_color[0]).abs() < 0.05
            && (rc[1] - current_color[1]).abs() < 0.05
            && (rc[2] - current_color[2]).abs() < 0.05;

        let swatch_box = container("")
            .width(16)
            .height(16)
            .style(move |_| container::Style {
                background: Some(rc_color.into()),
                border: border::rounded(3)
                    .width(if is_selected_rc { 2 } else { 1 })
                    .color(if is_selected_rc {
                        Color::WHITE
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }),
                ..Default::default()
            });

        let btn = button(swatch_box)
            .padding(1)
            .style(move |theme, status| style::button::transparent(theme, status, is_selected_rc))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SelectRecentColor(rc)),
            ));

        swatches_row = swatches_row.push(btn);
    }

    let palette_row = row![
        container(text("Recent:").size(11).font(style::AZERET_MONO)).width(55),
        swatches_row,
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut col = column![
        line_style_row,
        width_row,
        opacity_row,
        hex_row,
        rgb_row,
        palette_row,
    ]
    .spacing(8);

    // Extra geometry options for Trendline: rays & arrowheads
    if matches!(d.kind, DrawingKind::Trendline { .. }) {
        let extend_row = row![
            row![
                checkbox(d.extend_left).on_toggle(move |_| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::ToggleExtendLeft),
                    )
                }),
                text("Extend Left").size(11).font(style::AZERET_MONO),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                checkbox(d.extend_right).on_toggle(move |_| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::ToggleExtendRight),
                    )
                }),
                text("Extend Right").size(11).font(style::AZERET_MONO),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        ]
        .spacing(16);

        let arrow_row = row![
            row![
                checkbox(d.arrow_left).on_toggle(move |_| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::ToggleArrowLeft),
                    )
                }),
                text("Arrow Left").size(11).font(style::AZERET_MONO),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            row![
                checkbox(d.arrow_right).on_toggle(move |_| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::ToggleArrowRight),
                    )
                }),
                text("Arrow Right").size(11).font(style::AZERET_MONO),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        ]
        .spacing(16);

        col = col
            .push(rule::horizontal(1))
            .push(extend_row)
            .push(arrow_row);
    }

    // Extra options for Rectangle: Fill Opacity
    if matches!(d.kind, DrawingKind::Rectangle { .. }) {
        let fill_pct = (d.fill_opacity.unwrap_or(0.25) * 100.0).round() as u8;
        let fill_slider = slider(0..=100, fill_pct, move |new_pct| {
            Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetFillOpacity(
                    new_pct as f32 / 100.0,
                )),
            )
        })
        .step(5u8)
        .width(Length::Fixed(180.0));

        let fill_row = row![
            container(text("Fill:").size(11).font(style::AZERET_MONO)).width(55),
            fill_slider,
            text(format!("{}%", fill_pct))
                .size(10)
                .font(style::AZERET_MONO),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        col = col.push(rule::horizontal(1)).push(fill_row);
    }

    col.into()
}

fn view_text_tab<'a>(
    pane: pane_grid::Pane,
    state: &'a DrawingSettingsState,
) -> Element<'a, Message> {
    let d = &state.drawing;
    let annot = d.annotation.as_ref();

    let text_field = text_input("e.g. Weekly Open, 4H FVG...", &state.text_input)
        .on_input(move |val| {
            Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::TextChanged(val)),
            )
        })
        .width(Length::Fill)
        .padding(6)
        .style(|theme, status| style::validated_text_input(theme, status, true));

    // Font size buttons (10..18)
    let current_font_size = annot.map(|a| a.font_size).unwrap_or(12.0);
    let size_btn = |sz: f32, label: &'static str| {
        let is_active = (current_font_size - sz).abs() < 0.5;
        button(text(label).size(10).font(style::AZERET_MONO))
            .padding([3, 7])
            .style(move |theme, status| style::button::modifier(theme, status, is_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetFontSize(sz)),
            ))
    };

    let font_size_row = row![
        container(text("Size:").size(11).font(style::AZERET_MONO)).width(60),
        size_btn(10.0, "10"),
        size_btn(12.0, "12"),
        size_btn(14.0, "14"),
        size_btn(16.0, "16"),
        size_btn(18.0, "18"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Horizontal alignment
    let current_align_h = annot.map(|a| a.align_h).unwrap_or(TextAlignH::Left);
    let align_h_btn = |ah: TextAlignH, label: &'static str| {
        let is_active = current_align_h == ah;
        button(text(label).size(10).font(style::AZERET_MONO))
            .padding([3, 8])
            .style(move |theme, status| style::button::modifier(theme, status, is_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetTextAlignH(ah)),
            ))
    };

    let align_h_row = row![
        container(text("Align X:").size(11).font(style::AZERET_MONO)).width(60),
        align_h_btn(TextAlignH::Left, "Left"),
        align_h_btn(TextAlignH::Center, "Center"),
        align_h_btn(TextAlignH::Right, "Right"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Vertical alignment
    let current_align_v = annot.map(|a| a.align_v).unwrap_or(TextAlignV::Top);
    let align_v_btn = |av: TextAlignV, label: &'static str| {
        let is_active = current_align_v == av;
        button(text(label).size(10).font(style::AZERET_MONO))
            .padding([3, 8])
            .style(move |theme, status| style::button::modifier(theme, status, is_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::SetTextAlignV(av)),
            ))
    };

    let align_v_row = row![
        container(text("Align Y:").size(11).font(style::AZERET_MONO)).width(60),
        align_v_btn(TextAlignV::Top, "Top"),
        align_v_btn(TextAlignV::Center, "Center"),
        align_v_btn(TextAlignV::Bottom, "Bottom"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Text color
    let text_hex_field = text_input("#RRGGBB", &state.text_hex_input)
        .on_input(move |val| {
            Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::TextHexChanged(val)),
            )
        })
        .on_submit(Message::PaneEvent(
            pane,
            Event::DrawingSettingsAction(DrawingSettingsAction::ApplyTextHex),
        ))
        .width(Length::Fixed(90.0))
        .padding(4)
        .style(|theme, status| style::validated_text_input(theme, status, true));

    let text_color_row = row![
        container(text("Color:").size(11).font(style::AZERET_MONO)).width(60),
        text_hex_field,
        button(text("Apply").size(10).font(style::AZERET_MONO))
            .padding([3, 8])
            .style(|theme, status| style::button::confirm(theme, status, false))
            .on_press(Message::PaneEvent(
                pane,
                Event::DrawingSettingsAction(DrawingSettingsAction::ApplyTextHex),
            )),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    column![
        text("Text Comment / Label:")
            .size(11)
            .font(style::AZERET_MONO),
        text_field,
        font_size_row,
        align_h_row,
        align_v_row,
        text_color_row,
    ]
    .spacing(10)
    .into()
}

fn view_coordinates_tab<'a>(
    pane: pane_grid::Pane,
    state: &'a DrawingSettingsState,
) -> Element<'a, Message> {
    let d = &state.drawing;
    let mut rows = column![].spacing(8);

    match &d.kind {
        DrawingKind::HorizontalLine { .. } => {
            let p_input = text_input("Price", &state.coord_p1)
                .on_input(move |v| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::CoordP1Changed(v)),
                    )
                })
                .width(Length::Fixed(160.0))
                .padding(5)
                .style(|theme, status| style::validated_text_input(theme, status, true));

            rows = rows.push(
                row![
                    container(text("Price:").size(11).font(style::AZERET_MONO)).width(60),
                    p_input,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        DrawingKind::Trendline { .. } | DrawingKind::Rectangle { .. } => {
            let p1_input = text_input("Price 1", &state.coord_p1)
                .on_input(move |v| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::CoordP1Changed(v)),
                    )
                })
                .width(Length::Fixed(160.0))
                .padding(5)
                .style(|theme, status| style::validated_text_input(theme, status, true));

            let p2_input = text_input("Price 2", &state.coord_p2)
                .on_input(move |v| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::CoordP2Changed(v)),
                    )
                })
                .width(Length::Fixed(160.0))
                .padding(5)
                .style(|theme, status| style::validated_text_input(theme, status, true));

            let t1_input = text_input("Time 1 (ms)", &state.coord_t1)
                .on_input(move |v| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::CoordT1Changed(v)),
                    )
                })
                .width(Length::Fixed(160.0))
                .padding(5)
                .style(|theme, status| style::validated_text_input(theme, status, true));

            let t2_input = text_input("Time 2 (ms)", &state.coord_t2)
                .on_input(move |v| {
                    Message::PaneEvent(
                        pane,
                        Event::DrawingSettingsAction(DrawingSettingsAction::CoordT2Changed(v)),
                    )
                })
                .width(Length::Fixed(160.0))
                .padding(5)
                .style(|theme, status| style::validated_text_input(theme, status, true));

            rows = rows
                .push(
                    row![
                        container(text("Price 1:").size(11).font(style::AZERET_MONO)).width(60),
                        p1_input,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .push(
                    row![
                        container(text("Time 1:").size(11).font(style::AZERET_MONO)).width(60),
                        t1_input,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .push(
                    row![
                        container(text("Price 2:").size(11).font(style::AZERET_MONO)).width(60),
                        p2_input,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .push(
                    row![
                        container(text("Time 2:").size(11).font(style::AZERET_MONO)).width(60),
                        t2_input,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
        }
        _ => {
            rows = rows.push(text("Coordinates for this tool are edited on chart.").size(11));
        }
    }

    rows.into()
}

fn view_visibility_tab<'a>(
    pane: pane_grid::Pane,
    state: &'a DrawingSettingsState,
) -> Element<'a, Message> {
    let vis = state.drawing.visibility;

    let vis_row = |checked: bool, label: &'static str, action: DrawingSettingsAction| {
        row![
            checkbox(checked).on_toggle(move |_| {
                Message::PaneEvent(pane, Event::DrawingSettingsAction(action.clone()))
            }),
            text(label).size(11).font(style::AZERET_MONO),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
    };

    column![
        text("Show drawing on timeframes:")
            .size(12)
            .font(style::AZERET_MONO),
        vis_row(
            vis.seconds,
            "Seconds (S1)",
            DrawingSettingsAction::ToggleVisibilitySeconds
        ),
        vis_row(
            vis.minutes,
            "Minutes (1m, 3m, 5m, 15m, 30m)",
            DrawingSettingsAction::ToggleVisibilityMinutes
        ),
        vis_row(
            vis.hours,
            "Hours (1h, 2h, 4h, 6h, 8h, 12h)",
            DrawingSettingsAction::ToggleVisibilityHours
        ),
        vis_row(
            vis.days,
            "Days (1D, 3D)",
            DrawingSettingsAction::ToggleVisibilityDays
        ),
        vis_row(
            vis.weeks,
            "Weeks / Months (1W, 1M)",
            DrawingSettingsAction::ToggleVisibilityWeeks
        ),
    ]
    .spacing(10)
    .into()
}
