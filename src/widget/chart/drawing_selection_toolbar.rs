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
}

pub fn view<'a, Message: 'a + Clone>(
    drawing: &Drawing,
    show_settings: bool,
    on_action: impl Fn(SelectionToolbarAction) -> Message + 'a + Copy,
) -> Element<'a, Message> {
    // 1. Drag Handle
    let drag_handle = container(icon_text(Icon::DragHandle, 13).style(|theme: &Theme| {
        let palette = theme.extended_palette();
        iced::widget::text::Style {
            color: Some(palette.background.base.text.scale_alpha(0.4)),
        }
    }))
    .padding(padding::left(4).right(4))
    .align_y(Alignment::Center);

    // 2. Settings button (Cog)
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

    // 3. Lock button (Locked / Unlocked)
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

    // 4. Trash button (Delete this drawing)
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
        row![
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
            tip_settings,
            tip_lock,
            tip_delete,
        ]
        .spacing(4)
        .align_y(Alignment::Center)
    } else {
        row![drag_handle, tip_settings, tip_lock, tip_delete]
            .spacing(3)
            .align_y(Alignment::Center)
    };

    let mut main_column = column![main_row].spacing(4);

    if show_settings {
        let settings_content = view_settings(drawing, on_action);
        main_column = main_column.push(settings_content);
    }

    let mut main_container = container(main_column)
        .padding(padding::top(3).bottom(3).left(4).right(4))
        .style(toolbar_container_style);

    if show_settings {
        main_container = main_container.width(Length::Fixed(252.0));
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
