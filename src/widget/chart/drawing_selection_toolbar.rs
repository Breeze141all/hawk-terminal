use crate::style::{self, Icon, icon_text};
use crate::widget::chart::drawing_toolbar::{
    toolbar_button_style, toolbar_clear_button_style, toolbar_container_style,
};
use crate::widget::tooltip;
use data::chart::drawing::Drawing;
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Theme, border, padding};

#[derive(Debug, Clone)]
pub enum SelectionToolbarAction {
    ToggleSettings,
    ToggleLock,
    Delete,
    SetColor([f32; 4]),
    SetWidth(f32),
    SetPositionProfitColor([f32; 4]),
    SetPositionStopColor([f32; 4]),
    SetPositionEntryColor([f32; 4]),
    AutofillJournal,
}

pub fn view<'a, Message: 'a + Clone>(
    drawing: &Drawing,
    show_settings: bool,
    on_action: impl Fn(SelectionToolbarAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    let is_position = matches!(
        drawing.kind,
        data::chart::drawing::DrawingKind::Position { .. }
    );

    // 1. Drag Handle
    let drag_handle = container(icon_text(Icon::DragHandle, 13).style(|theme: &Theme| {
        let palette = theme.extended_palette();
        iced::widget::text::Style {
            color: Some(palette.background.base.text.scale_alpha(0.4)),
        }
    }))
    .padding(padding::left(4).right(4))
    .align_y(Alignment::Center);

    // 2. Journal button (Position only: Add to journal)
    let journal_btn = if is_position {
        let btn = button(icon_text(Icon::Journal, 14))
            .padding(4)
            .style(|theme: &Theme, status| toolbar_button_style(theme, status, false))
            .on_press(on_action(SelectionToolbarAction::AutofillJournal));
        Some(tooltip(
            btn,
            Some("Add to Journal (Shift+G)"),
            iced::widget::tooltip::Position::Top,
        ))
    } else {
        None
    };

    // 3. Settings button (Cog)
    let settings_icon = icon_text(Icon::Cog, 14);
    let settings_btn = button(settings_icon)
        .padding(4)
        .style(move |theme: &Theme, status| toolbar_button_style(theme, status, show_settings))
        .on_press(on_action(SelectionToolbarAction::ToggleSettings));
    let tip_settings = tooltip(
        settings_btn,
        Some("Settings"),
        iced::widget::tooltip::Position::Top,
    );

    // 4. Lock button (Locked / Unlocked)
    let is_locked = drawing.is_locked;
    let lock_icon = icon_text(
        if is_locked {
            Icon::Locked
        } else {
            Icon::Unlocked
        },
        14,
    );
    let lock_btn = button(lock_icon)
        .padding(4)
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            if is_locked {
                button::Style {
                    background: Some(
                        palette
                            .warning
                            .base
                            .color
                            .scale_alpha(if palette.is_dark { 0.25 } else { 0.2 })
                            .into(),
                    ),
                    text_color: palette.warning.base.color,
                    border: iced::Border {
                        radius: 4.0.into(),
                        width: 1.0,
                        color: palette.warning.base.color,
                    },
                    ..Default::default()
                }
            } else {
                toolbar_button_style(theme, status, false)
            }
        })
        .on_press(on_action(SelectionToolbarAction::ToggleLock));
    let tip_lock = tooltip(
        lock_btn,
        Some(if is_locked {
            "Unlock Drawing"
        } else {
            "Lock Drawing"
        }),
        iced::widget::tooltip::Position::Top,
    );

    // 5. Trash button (Delete this drawing)
    let trash_icon = icon_text(Icon::TrashBin, 14);
    let delete_btn = button(trash_icon)
        .padding(4)
        .style(|theme: &Theme, status| toolbar_clear_button_style(theme, status))
        .on_press(on_action(SelectionToolbarAction::Delete));
    let tip_delete = tooltip(
        delete_btn,
        Some("Delete Drawing"),
        iced::widget::tooltip::Position::Top,
    );

    let tool_name = match &drawing.kind {
        data::chart::drawing::DrawingKind::Brush { .. } => "Brush",
        data::chart::drawing::DrawingKind::Rectangle { .. } => "Rectangle",
        data::chart::drawing::DrawingKind::Trendline { .. } => "Trendline",
        data::chart::drawing::DrawingKind::HorizontalLine { .. } => "Line",
        data::chart::drawing::DrawingKind::Path { .. } => "Path",
        data::chart::drawing::DrawingKind::Position { is_long: true, .. } => "Long",
        data::chart::drawing::DrawingKind::Position { is_long: false, .. } => "Short",
    };

    let main_row = if show_settings {
        let mut r = row![
            drag_handle,
            text(tool_name)
                .size(10)
                .font(style::AZERET_MONO)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    iced::widget::text::Style {
                        color: Some(palette.background.base.text.scale_alpha(0.6)),
                    }
                }),
            iced::widget::space::horizontal(),
        ];
        if let Some(jb) = journal_btn {
            r = r.push(jb);
        }
        r.push(tip_settings)
            .push(tip_lock)
            .push(tip_delete)
            .spacing(4)
            .align_y(Alignment::Center)
    } else {
        let mut r = row![drag_handle];
        if let Some(jb) = journal_btn {
            r = r.push(jb);
        }
        r.push(tip_settings)
            .push(tip_lock)
            .push(tip_delete)
            .spacing(3)
            .align_y(Alignment::Center)
    };

    let mut main_column = column![main_row].spacing(4);

    if show_settings {
        let settings_content = if is_position {
            view_position_settings(drawing, on_action)
        } else {
            view_settings(drawing, on_action)
        };
        main_column = main_column.push(settings_content);
    }

    let mut main_container = container(main_column)
        .padding(padding::top(3).bottom(3).left(4).right(4))
        .style(toolbar_container_style);

    if show_settings {
        main_container =
            main_container.width(Length::Fixed(if is_position { 260.0 } else { 252.0 }));
    }

    main_container.into()
}

fn view_settings<'a, Message: 'a + Clone>(
    drawing: &Drawing,
    on_action: impl Fn(SelectionToolbarAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    let current_alpha = drawing.color[3];
    let current_width = drawing.width;

    // Palette swatches
    let preset_colors: [([f32; 3], &'static str); 8] = [
        ([1.0, 1.0, 1.0], "White"),
        ([0.65, 0.65, 0.65], "Gray"),
        ([0.94, 0.27, 0.24], "Red"),
        ([1.0, 0.60, 0.0], "Orange"),
        ([1.0, 0.92, 0.23], "Yellow"),
        ([0.15, 0.68, 0.38], "Green"),
        ([0.16, 0.38, 1.0], "Blue"),
        ([0.67, 0.28, 0.74], "Purple"),
    ];

    let mut color_swatches = row![].spacing(3).align_y(Alignment::Center);
    for (rgb, label) in preset_colors {
        let is_selected_color = (drawing.color[0] - rgb[0]).abs() < 0.05
            && (drawing.color[1] - rgb[1]).abs() < 0.05
            && (drawing.color[2] - rgb[2]).abs() < 0.05;

        let swatch_color = Color::from_rgb(rgb[0], rgb[1], rgb[2]);
        let swatch_box = container("")
            .width(14)
            .height(14)
            .style(move |_theme: &Theme| container::Style {
                background: Some(swatch_color.into()),
                border: border::rounded(3)
                    .width(if is_selected_color { 2 } else { 1 })
                    .color(if is_selected_color {
                        Color::WHITE
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }),
                ..Default::default()
            });

        let swatch_btn = button(swatch_box)
            .padding(2)
            .style(move |theme: &Theme, status| {
                toolbar_button_style(theme, status, is_selected_color)
            })
            .on_press(on_action(SelectionToolbarAction::SetColor([
                rgb[0],
                rgb[1],
                rgb[2],
                current_alpha,
            ])));

        color_swatches = color_swatches.push(tooltip(
            swatch_btn,
            Some(label),
            iced::widget::tooltip::Position::Top,
        ));
    }

    let color_row = row![
        container(text("Color:").size(10).font(style::AZERET_MONO)).width(52),
        color_swatches,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Line thickness buttons
    let widths = [1.0, 2.0, 3.0, 4.0, 6.0];
    let mut width_buttons = row![].spacing(3).align_y(Alignment::Center);

    for w in widths {
        let is_active = (current_width - w).abs() < 0.4;
        let w_label = format!("{:.0}px", w);
        let btn = button(
            text(w_label)
                .size(10)
                .font(style::AZERET_MONO)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .padding(padding::top(2).bottom(2).left(5).right(5))
        .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_active))
        .on_press(on_action(SelectionToolbarAction::SetWidth(w)));

        width_buttons = width_buttons.push(btn);
    }

    let width_row = row![
        container(text("Width:").size(10).font(style::AZERET_MONO)).width(52),
        width_buttons,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Opacity / Transparency slider
    let alpha_pct = (current_alpha * 100.0).round() as u8;
    let r = drawing.color[0];
    let g = drawing.color[1];
    let b = drawing.color[2];

    let opacity_slider = iced::widget::slider(10..=100, alpha_pct, move |new_pct| {
        let new_alpha = new_pct as f32 / 100.0;
        on_action(SelectionToolbarAction::SetColor([r, g, b, new_alpha]))
    })
    .step(5u8)
    .width(Length::Fixed(120.0));

    let opacity_row = row![
        container(text("Opacity:").size(10).font(style::AZERET_MONO)).width(52),
        opacity_slider,
        container(
            text(format!("{}%", alpha_pct))
                .size(10)
                .font(style::AZERET_MONO)
        )
        .width(36)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Separator line
    let separator = container("")
        .width(Length::Fill)
        .height(1)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.background.weak.color.into()),
                ..Default::default()
            }
        });

    column![separator, color_row, width_row, opacity_row,]
        .spacing(6)
        .padding(padding::top(4).bottom(2).left(2).right(2))
        .into()
}

fn view_position_settings<'a, Message: 'a + Clone>(
    drawing: &Drawing,
    on_action: impl Fn(SelectionToolbarAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    let pos_style = drawing.position_style();
    let current_width = drawing.width;

    let preset_colors: [([f32; 3], &'static str); 8] = [
        ([1.0, 1.0, 1.0], "White"),
        ([0.65, 0.65, 0.65], "Gray"),
        ([0.94, 0.27, 0.24], "Red"),
        ([1.0, 0.60, 0.0], "Orange"),
        ([1.0, 0.92, 0.23], "Yellow"),
        ([0.15, 0.68, 0.38], "Green"),
        ([0.16, 0.38, 1.0], "Blue"),
        ([0.67, 0.28, 0.74], "Purple"),
    ];

    let separator = || {
        container("")
            .width(Length::Fill)
            .height(1)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.weak.color.into()),
                    ..Default::default()
                }
            })
    };

    // --- Take Profit Section ---
    let tp_rgb = [
        pos_style.profit_color[0],
        pos_style.profit_color[1],
        pos_style.profit_color[2],
    ];
    let tp_alpha = pos_style.profit_color[3];
    let tp_alpha_pct = (tp_alpha * 100.0).round() as u8;

    let mut tp_swatches = row![].spacing(3).align_y(Alignment::Center);
    for (rgb, label) in preset_colors {
        let is_selected = (tp_rgb[0] - rgb[0]).abs() < 0.05
            && (tp_rgb[1] - rgb[1]).abs() < 0.05
            && (tp_rgb[2] - rgb[2]).abs() < 0.05;

        let swatch_color = Color::from_rgb(rgb[0], rgb[1], rgb[2]);
        let swatch_box = container("")
            .width(14)
            .height(14)
            .style(move |_theme: &Theme| container::Style {
                background: Some(swatch_color.into()),
                border: border::rounded(3)
                    .width(if is_selected { 2 } else { 1 })
                    .color(if is_selected {
                        Color::WHITE
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }),
                ..Default::default()
            });

        let swatch_btn = button(swatch_box)
            .padding(2)
            .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_selected))
            .on_press(on_action(SelectionToolbarAction::SetPositionProfitColor([
                rgb[0], rgb[1], rgb[2], tp_alpha,
            ])));

        tp_swatches = tp_swatches.push(tooltip(
            swatch_btn,
            Some(label),
            iced::widget::tooltip::Position::Top,
        ));
    }

    let tp_color_row = row![
        container(text("Take:").size(10).font(style::AZERET_MONO)).width(48),
        tp_swatches,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let tp_opacity_slider = iced::widget::slider(0..=100, tp_alpha_pct, move |new_pct| {
        let new_alpha = new_pct as f32 / 100.0;
        on_action(SelectionToolbarAction::SetPositionProfitColor([
            tp_rgb[0], tp_rgb[1], tp_rgb[2], new_alpha,
        ]))
    })
    .step(5u8)
    .width(Length::Fixed(120.0));

    let tp_opacity_row = row![
        container(text("Opacity:").size(10).font(style::AZERET_MONO)).width(48),
        tp_opacity_slider,
        container(
            text(format!("{}%", tp_alpha_pct))
                .size(10)
                .font(style::AZERET_MONO)
        )
        .width(36)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // --- Stop Loss Section ---
    let sl_rgb = [
        pos_style.stop_color[0],
        pos_style.stop_color[1],
        pos_style.stop_color[2],
    ];
    let sl_alpha = pos_style.stop_color[3];
    let sl_alpha_pct = (sl_alpha * 100.0).round() as u8;

    let mut sl_swatches = row![].spacing(3).align_y(Alignment::Center);
    for (rgb, label) in preset_colors {
        let is_selected = (sl_rgb[0] - rgb[0]).abs() < 0.05
            && (sl_rgb[1] - rgb[1]).abs() < 0.05
            && (sl_rgb[2] - rgb[2]).abs() < 0.05;

        let swatch_color = Color::from_rgb(rgb[0], rgb[1], rgb[2]);
        let swatch_box = container("")
            .width(14)
            .height(14)
            .style(move |_theme: &Theme| container::Style {
                background: Some(swatch_color.into()),
                border: border::rounded(3)
                    .width(if is_selected { 2 } else { 1 })
                    .color(if is_selected {
                        Color::WHITE
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }),
                ..Default::default()
            });

        let swatch_btn = button(swatch_box)
            .padding(2)
            .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_selected))
            .on_press(on_action(SelectionToolbarAction::SetPositionStopColor([
                rgb[0], rgb[1], rgb[2], sl_alpha,
            ])));

        sl_swatches = sl_swatches.push(tooltip(
            swatch_btn,
            Some(label),
            iced::widget::tooltip::Position::Top,
        ));
    }

    let sl_color_row = row![
        container(text("Stop:").size(10).font(style::AZERET_MONO)).width(48),
        sl_swatches,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let sl_opacity_slider = iced::widget::slider(0..=100, sl_alpha_pct, move |new_pct| {
        let new_alpha = new_pct as f32 / 100.0;
        on_action(SelectionToolbarAction::SetPositionStopColor([
            sl_rgb[0], sl_rgb[1], sl_rgb[2], new_alpha,
        ]))
    })
    .step(5u8)
    .width(Length::Fixed(120.0));

    let sl_opacity_row = row![
        container(text("Opacity:").size(10).font(style::AZERET_MONO)).width(48),
        sl_opacity_slider,
        container(
            text(format!("{}%", sl_alpha_pct))
                .size(10)
                .font(style::AZERET_MONO)
        )
        .width(36)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // --- Entry Line Section ---
    let entry_rgb = [
        pos_style.entry_color[0],
        pos_style.entry_color[1],
        pos_style.entry_color[2],
    ];
    let mut entry_swatches = row![].spacing(3).align_y(Alignment::Center);
    for (rgb, label) in preset_colors {
        let is_selected = (entry_rgb[0] - rgb[0]).abs() < 0.05
            && (entry_rgb[1] - rgb[1]).abs() < 0.05
            && (entry_rgb[2] - rgb[2]).abs() < 0.05;

        let swatch_color = Color::from_rgb(rgb[0], rgb[1], rgb[2]);
        let swatch_box = container("")
            .width(14)
            .height(14)
            .style(move |_theme: &Theme| container::Style {
                background: Some(swatch_color.into()),
                border: border::rounded(3)
                    .width(if is_selected { 2 } else { 1 })
                    .color(if is_selected {
                        Color::WHITE
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.4)
                    }),
                ..Default::default()
            });

        let swatch_btn = button(swatch_box)
            .padding(2)
            .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_selected))
            .on_press(on_action(SelectionToolbarAction::SetPositionEntryColor([
                rgb[0],
                rgb[1],
                rgb[2],
                pos_style.entry_color[3],
            ])));

        entry_swatches = entry_swatches.push(tooltip(
            swatch_btn,
            Some(label),
            iced::widget::tooltip::Position::Top,
        ));
    }

    let entry_color_row = row![
        container(text("Entry:").size(10).font(style::AZERET_MONO)).width(48),
        entry_swatches,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    // Line thickness buttons
    let widths = [1.0, 2.0, 3.0, 4.0];
    let mut width_buttons = row![].spacing(3).align_y(Alignment::Center);
    for w in widths {
        let is_active = (current_width - w).abs() < 0.4;
        let w_label = format!("{:.0}px", w);
        let btn = button(
            text(w_label)
                .size(10)
                .font(style::AZERET_MONO)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .padding(padding::top(2).bottom(2).left(5).right(5))
        .style(move |theme: &Theme, status| toolbar_button_style(theme, status, is_active))
        .on_press(on_action(SelectionToolbarAction::SetWidth(w)));

        width_buttons = width_buttons.push(btn);
    }

    let width_row = row![
        container(text("Width:").size(10).font(style::AZERET_MONO)).width(48),
        width_buttons,
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    column![
        separator(),
        tp_color_row,
        tp_opacity_row,
        separator(),
        sl_color_row,
        sl_opacity_row,
        separator(),
        entry_color_row,
        width_row,
    ]
    .spacing(5)
    .padding(padding::top(4).bottom(2).left(2).right(2))
    .into()
}
