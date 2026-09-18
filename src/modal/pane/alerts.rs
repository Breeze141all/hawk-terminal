use crate::screen::dashboard::pane::{Event, Message};
use crate::style::{self, Icon, icon_text};
use data::chart::alert::{AlertCondition, AlertFilter, AlertStatus, PriceAlert};
use iced::widget::{
    button, column, container, pane_grid, pick_list, row, rule, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Length};

pub fn alerts_view<'a>(
    pane: pane_grid::Pane,
    ticker_symbol: &str,
    current_price: Option<f32>,
    this_chart_alerts: &[PriceAlert],
    all_alerts: &[PriceAlert],
    filter: AlertFilter,
    price_input: &'a str,
    condition: AlertCondition,
) -> Element<'a, Message> {
    let title = row![
        text(format!("Price Alerts - {ticker_symbol}")).size(15),
        space(),
        button(icon_text(Icon::Close, 11))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(Message::PaneEvent(pane, Event::HideModal))
            .padding([2, 4]),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let price_label = if let Some(cp) = current_price {
        format!("Current: {:.2}", cp)
    } else {
        "Current: --".to_string()
    };

    let current_price_display = text(price_label).size(12);

    let input = text_input("Target Price...", price_input)
        .on_input(move |val| Message::PaneEvent(pane, Event::AlertPriceInput(val)))
        .width(Length::Fill)
        .padding(6)
        .style(|theme, status| style::validated_text_input(theme, status, true));

    let condition_picklist = pick_list(
        &[
            AlertCondition::CrossAbove,
            AlertCondition::CrossBelow,
            AlertCondition::Crossing,
        ][..],
        Some(condition),
        move |cond| Message::PaneEvent(pane, Event::AlertConditionSelected(cond)),
    )
    .width(Length::Fill);

    let can_add = price_input.trim().parse::<f32>().is_ok();
    let add_btn = button(
        row![icon_text(Icon::Checkmark, 11), text(" Add Alert").size(12)]
            .align_y(Alignment::Center),
    )
    .style(move |theme, status| style::button::confirm(theme, status, false))
    .padding([5, 12])
    .on_press_maybe(if can_add {
        let price = price_input.trim().parse::<f32>().unwrap_or(0.0);
        Some(Message::PaneEvent(pane, Event::AddPriceAlert(price)))
    } else {
        None
    });

    let add_section = column![
        current_price_display,
        row![input, condition_picklist].spacing(6),
        add_btn,
    ]
    .spacing(8);

    let divider = rule::horizontal(1);

    // Filter tabs
    let this_chart_active = filter == AlertFilter::ThisChart;
    let all_charts_active = filter == AlertFilter::AllCharts;

    let filter_tabs = row![
        button(text(format!("This Chart ({})", this_chart_alerts.len())).size(11))
            .style(move |theme, status| style::button::modifier(theme, status, this_chart_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::AlertFilterSelected(AlertFilter::ThisChart)
            ))
            .padding([3, 8]),
        button(text(format!("All Alerts ({})", all_alerts.len())).size(11))
            .style(move |theme, status| style::button::modifier(theme, status, all_charts_active))
            .on_press(Message::PaneEvent(
                pane,
                Event::AlertFilterSelected(AlertFilter::AllCharts)
            ))
            .padding([3, 8]),
        space(),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let displayed_alerts: &[PriceAlert] = match filter {
        AlertFilter::ThisChart => this_chart_alerts,
        AlertFilter::AllCharts => all_alerts,
    };

    let has_triggered = displayed_alerts
        .iter()
        .any(|a| a.status == AlertStatus::Triggered);

    let filter_and_actions = if has_triggered {
        row![
            filter_tabs,
            button(text("Clear Triggered").size(10))
                .style(|theme, status| style::button::cancel(theme, status, false))
                .on_press(Message::PaneEvent(pane, Event::ClearTriggeredAlerts))
                .padding([2, 6]),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
    } else {
        row![filter_tabs].width(Length::Fill)
    };

    let mut alerts_list = column![].spacing(6);

    if displayed_alerts.is_empty() {
        let empty_text = match filter {
            AlertFilter::ThisChart => "No alerts for this chart.",
            AlertFilter::AllCharts => "No alerts recorded yet.",
        };
        alerts_list = alerts_list.push(text(empty_text).size(12));
    } else {
        for alert in displayed_alerts {
            let id = alert.id;
            let status_color = match alert.status {
                AlertStatus::Active => iced::Color::from_rgb(0.2, 0.8, 0.4),
                AlertStatus::Triggered => iced::Color::from_rgb(0.9, 0.6, 0.1),
                AlertStatus::Muted => iced::Color::from_rgb(0.5, 0.5, 0.5),
            };

            let status_text = match alert.status {
                AlertStatus::Active => "ACTIVE".to_string(),
                AlertStatus::Triggered => {
                    if let Some(time) = alert.triggered_at {
                        chrono::DateTime::from_timestamp_millis(time as i64)
                            .map(|dt| dt.format("%H:%M:%S").to_string())
                            .unwrap_or_else(|| "TRIGGERED".to_string())
                    } else {
                        "TRIGGERED".to_string()
                    }
                }
                AlertStatus::Muted => "MUTED".to_string(),
            };

            let mut item_row = row![].align_y(Alignment::Center).spacing(6);

            if filter == AlertFilter::AllCharts {
                item_row = item_row.push(
                    container(
                        text(alert.ticker_symbol.clone())
                            .size(10)
                            .font(style::AZERET_MONO),
                    )
                    .padding([1, 4])
                    .style(|theme: &iced::Theme| {
                        let p = theme.extended_palette();
                        container::Style {
                            background: Some(p.background.weak.color.into()),
                            text_color: Some(p.background.base.text),
                            border: iced::Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                );
            }

            item_row = item_row.push(text(format!("{:.2}", alert.target_price)).size(12));
            item_row = item_row.push(text(format!("({})", alert.condition)).size(10));
            item_row = item_row.push(space());
            item_row = item_row.push(text(status_text).size(10).color(status_color));

            item_row = item_row.push(
                button(icon_text(
                    if alert.status == AlertStatus::Muted {
                        Icon::SpeakerHigh
                    } else {
                        Icon::SpeakerOff
                    },
                    11,
                ))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(Message::PaneEvent(pane, Event::TogglePriceAlert(id)))
                .padding([2, 4]),
            );

            item_row = item_row.push(
                button(icon_text(Icon::TrashBin, 11))
                    .style(|theme, status| style::button::cancel(theme, status, false))
                    .on_press(Message::PaneEvent(pane, Event::DeletePriceAlert(id)))
                    .padding([2, 4]),
            );

            alerts_list = alerts_list.push(item_row);
        }
    }

    let scrollable_alerts = scrollable(alerts_list).height(Length::Fixed(160.0));

    let content = column![
        title,
        add_section,
        divider,
        filter_and_actions,
        scrollable_alerts
    ]
    .spacing(10)
    .width(Length::Fixed(350.0));

    container(content)
        .padding(16)
        .style(style::chart_modal)
        .into()
}
