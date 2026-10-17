use crate::screen::ConfirmDialog;
use crate::screen::dashboard::sidebar as dashboard_sidebar;
use crate::style::{self, Icon, icon_text};
use crate::widget::scrollable_content;
use crate::{Message, tooltip};
use data::config::theme::{breez_theme, deeptrades_theme, default_theme, flowsurface_legacy_theme};
use data::sidebar;
use data::{JournalMode, ScaleFactor, Theme, UserTimezone};
use exchange::SizeUnit;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, space, text,
    tooltip::Position as TooltipPosition,
};
use iced::{Alignment, Element, Length, padding};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlobalSettingsCategory {
    #[default]
    General,
    Appearance,
    MarketData,
    System,
}

impl GlobalSettingsCategory {
    pub const ALL: [GlobalSettingsCategory; 4] = [
        GlobalSettingsCategory::General,
        GlobalSettingsCategory::Appearance,
        GlobalSettingsCategory::MarketData,
        GlobalSettingsCategory::System,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            GlobalSettingsCategory::General => "General",
            GlobalSettingsCategory::Appearance => "Appearance",
            GlobalSettingsCategory::MarketData => "Market Data",
            GlobalSettingsCategory::System => "System & Storage",
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            GlobalSettingsCategory::General => Icon::Cog,
            GlobalSettingsCategory::Appearance => Icon::StarFilled,
            GlobalSettingsCategory::MarketData => Icon::ChartOutline,
            GlobalSettingsCategory::System => Icon::Folder,
        }
    }
}

pub fn global_settings_modal_view<'a>(
    selected_category: GlobalSettingsCategory,
    timezone: UserTimezone,
    sidebar_pos: sidebar::Position,
    journal_mode: JournalMode,
    volume_size_unit: SizeUnit,
    theme: &'a Theme,
    custom_theme: Option<&'a iced::Theme>,
    ui_scale_factor: ScaleFactor,
    #[cfg(feature = "volume-search")] show_volume_search: bool,
    cache_market_metadata: bool,
    is_metadata_loading: bool,
    last_metadata_update: Option<chrono::DateTime<chrono::Utc>>,
) -> Element<'a, Message> {
    // Left Sidebar: Category navigation
    let mut sidebar_col = column![
        text("Settings").size(14),
        rule::horizontal(1.0).style(style::split_ruler),
    ]
    .spacing(8);

    let mut cat_buttons = column![].spacing(3);
    for cat in GlobalSettingsCategory::ALL {
        let is_selected = cat == selected_category;

        let btn_label = row![icon_text(cat.icon(), 13), text(cat.title()).size(12),]
            .spacing(8)
            .align_y(Alignment::Center);

        let btn = button(btn_label)
            .width(Length::Fill)
            .padding([6, 8])
            .on_press(Message::SelectGlobalSettingsCategory(cat))
            .style(move |theme, status| {
                let palette = theme.extended_palette();
                iced::widget::button::Style {
                    text_color: if is_selected {
                        palette.primary.base.color
                    } else {
                        palette.background.base.text
                    },
                    border: iced::Border {
                        radius: 4.0.into(),
                        width: if is_selected { 1.0 } else { 0.0 },
                        color: if is_selected {
                            palette.primary.strong.color
                        } else {
                            iced::Color::TRANSPARENT
                        },
                    },
                    background: match status {
                        iced::widget::button::Status::Active => {
                            if is_selected {
                                Some(palette.background.weak.color.into())
                            } else {
                                None
                            }
                        }
                        iced::widget::button::Status::Hovered => {
                            Some(palette.background.weak.color.into())
                        }
                        iced::widget::button::Status::Pressed => {
                            Some(palette.background.strong.color.into())
                        }
                        iced::widget::button::Status::Disabled => None,
                    },
                    ..Default::default()
                }
            });

        cat_buttons = cat_buttons.push(btn);
    }

    sidebar_col = sidebar_col.push(cat_buttons);

    let sidebar = container(sidebar_col)
        .width(Length::Fixed(150.0))
        .height(Length::Fill)
        .padding(padding::right(10));

    // Right Content Area
    let right_header = row![text(selected_category.title()).size(14),].align_y(Alignment::Center);

    let category_content: Element<'a, Message> = match selected_category {
        GlobalSettingsCategory::General => {
            let timezone_picklist = pick_list(
                [UserTimezone::Utc, UserTimezone::Local],
                Some(timezone),
                Message::SetTimezone,
            );

            let timezone_section = column![
                text("Time Zone").size(13),
                text("Controls timestamp display across price charts, tape, and alert logs")
                    .size(11),
                timezone_picklist,
            ]
            .spacing(4);

            let sidebar_picklist = pick_list(
                [sidebar::Position::Left, sidebar::Position::Right],
                Some(sidebar_pos),
                |pos| Message::Sidebar(dashboard_sidebar::Message::SetSidebarPosition(pos)),
            );

            let sidebar_section = column![
                text("Sidebar Position").size(13),
                text("Dock primary navigation sidebar to left or right edge of screen").size(11),
                sidebar_picklist,
            ]
            .spacing(4);

            let journal_picklist = pick_list(
                JournalMode::ALL,
                Some(journal_mode),
                Message::SetJournalMode,
            );

            let journal_section = column![
                text("Trade Journal Recording").size(13),
                text("Auto mode captures filled positions and PnL metrics into SQLite database")
                    .size(11),
                journal_picklist,
            ]
            .spacing(4);

            column![
                container(timezone_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(sidebar_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(journal_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .into()
        }
        GlobalSettingsCategory::Appearance => {
            let theme_picklist = {
                let default_theme = iced_core::Theme::Custom(default_theme().into());
                let breez = iced_core::Theme::Custom(breez_theme().into());
                let deeptrades = iced_core::Theme::Custom(deeptrades_theme().into());
                let flowsurface_classic =
                    iced_core::Theme::Custom(flowsurface_legacy_theme().into());

                let mut themes: Vec<iced::Theme> =
                    vec![default_theme, breez, deeptrades, flowsurface_classic];

                if let Some(ct) = custom_theme {
                    themes.push(ct.clone());
                }

                themes.extend(iced_core::Theme::ALL.iter().cloned());

                pick_list(themes, Some(theme.0.clone()), |t| {
                    Message::ThemeSelected(Theme(t))
                })
            };

            let theme_section = column![
                text("Application Theme").size(13),
                text("Master visual theme and color palette for all charts and panels").size(11),
                theme_picklist,
            ]
            .spacing(4);

            let toggle_theme_editor = button(text("Open Theme Editor").size(12))
                .padding([6, 12])
                .on_press(Message::Sidebar(
                    dashboard_sidebar::Message::ToggleSidebarMenu(Some(sidebar::Menu::ThemeEditor)),
                ));

            let editor_section = column![
                text("Palette & Theme Editor").size(13),
                text("Customize background hues, chart canvas colors, and grid lines").size(11),
                toggle_theme_editor,
            ]
            .spacing(4);

            let scale_section = {
                let current_value: f32 = ui_scale_factor.into();

                let decrease_btn = if current_value > data::config::MIN_SCALE {
                    button(text("-").size(12))
                        .on_press(Message::ScaleFactorChanged((current_value - 0.1).into()))
                } else {
                    button(text("-").size(12))
                };

                let increase_btn = if current_value < data::config::MAX_SCALE {
                    button(text("+").size(12))
                        .on_press(Message::ScaleFactorChanged((current_value + 0.1).into()))
                } else {
                    button(text("+").size(12))
                };

                let stepper = container(
                    row![
                        decrease_btn,
                        text(format!("{:.0}%", current_value * 100.0)).size(13),
                        increase_btn,
                    ]
                    .align_y(Alignment::Center)
                    .spacing(8)
                    .padding(4),
                )
                .style(style::modal_container);

                let presets = row![
                    button(text("80%").size(11))
                        .padding([4, 6])
                        .on_press(Message::ScaleFactorChanged(0.8.into())),
                    button(text("90%").size(11))
                        .padding([4, 6])
                        .on_press(Message::ScaleFactorChanged(0.9.into())),
                    button(text("100%").size(11))
                        .padding([4, 6])
                        .on_press(Message::ScaleFactorChanged(1.0.into())),
                    button(text("110%").size(11))
                        .padding([4, 6])
                        .on_press(Message::ScaleFactorChanged(1.1.into())),
                    button(text("125%").size(11))
                        .padding([4, 6])
                        .on_press(Message::ScaleFactorChanged(1.25.into())),
                ]
                .spacing(4);

                column![
                    text("Interface Scale").size(13),
                    text("Adjust display zoom factor for high-DPI or compact displays").size(11),
                    row![stepper, space::horizontal(), presets]
                        .align_y(Alignment::Center)
                        .spacing(8),
                ]
                .spacing(6)
            };

            column![
                container(theme_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(editor_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(scale_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .into()
        }
        GlobalSettingsCategory::MarketData => {
            let size_in_quote_currency_checkbox = {
                let is_active = match volume_size_unit {
                    SizeUnit::Quote => true,
                    SizeUnit::Base => false,
                };

                let checkbox = checkbox(is_active)
                    .label("Size in quote currency (USD)")
                    .on_toggle(|checked| {
                        let on_dialog_confirm = Message::ApplyVolumeSizeUnit(if checked {
                            SizeUnit::Quote
                        } else {
                            SizeUnit::Base
                        });

                        let confirm_dialog = ConfirmDialog::new(
                            "Changing size display currency requires application restart"
                                .to_string(),
                            Box::new(on_dialog_confirm),
                        )
                        .with_confirm_btn_text("Restart now".to_string());

                        Message::ToggleDialogModal(Some(confirm_dialog))
                    });

                tooltip(
                    checkbox,
                    Some(
                        "Display sizes/volumes in quote currency (USD)\nHas no effect on inverse perps or open interest",
                    ),
                    TooltipPosition::Top,
                )
            };

            let display_unit_section = column![
                text("Display Currency").size(13),
                text("Convert traded volumes and ladder depths into quote USD equivalent").size(11),
                size_in_quote_currency_checkbox,
            ]
            .spacing(6);

            let trade_fetch_checkbox = {
                let is_active = exchange::fetcher::is_trade_fetch_enabled();

                let checkbox = checkbox(is_active)
                    .label("Fetch historical trades")
                    .on_toggle(|checked| {
                        if checked {
                            let confirm_dialog = ConfirmDialog::new(
                                "Fetching historical trades may download large archives and take some time to complete. Proceed?"
                                    .to_string(),
                                Box::new(Message::ToggleTradeFetch(true)),
                            );
                            Message::ToggleDialogModal(Some(confirm_dialog))
                        } else {
                            Message::ToggleTradeFetch(false)
                        }
                    });

                tooltip(
                    checkbox,
                    Some(
                        "Fetch historical and intraday trades for footprint charts (Binance, Bybit, OKX, Hyperliquid)",
                    ),
                    TooltipPosition::Top,
                )
            };

            let fetch_section = column![
                text("Historical Trade Ingestion").size(13),
                text(
                    "Download intraday tick archives for deep footprint replay and volume profiles"
                )
                .size(11),
                trade_fetch_checkbox,
            ]
            .spacing(8);

            let feeds_info = column![
                text("Connected Adapters").size(13),
                text("Multi-exchange market data streaming via high-throughput WebSockets:")
                    .size(11),
                column![
                    text("• Binance: Spot & Linear USDT Futures (Depth + AggTrades)").size(11),
                    text("• Bybit: Linear Perps (Orderbook L2 + Public Trades)").size(11),
                    text("• OKX: Linear Swaps (BBO + Trades + Open Interest)").size(11),
                    text("• Hyperliquid: Dual WS Streaming with instant L2 sync").size(11),
                ]
                .spacing(3)
                .padding(padding::left(6)),
            ]
            .spacing(6);

            let metadata_cache_section = {
                let checkbox = tooltip(
                    checkbox(cache_market_metadata)
                        .label("Cache market metadata")
                        .on_toggle(Message::SetMetadataCaching),
                    Some("Cache market metadata (tick sizes, lot quantities) for instant startup"),
                    TooltipPosition::Top,
                );

                if !cache_market_metadata {
                    column![
                        text("Startup Metadata Caching").size(13),
                        text("Persist parsed instrument rules to disk to start without API roundtrips").size(11),
                        checkbox,
                    ]
                    .spacing(6)
                } else {
                    let refresh_btn = {
                        let btn = button(text("Refresh now").size(11))
                            .style(move |theme, status| {
                                style::button::modifier(theme, status, is_metadata_loading)
                            });
                        let btn = if is_metadata_loading {
                            btn
                        } else {
                            btn.on_press(Message::RefreshMetadata)
                        };

                        tooltip(
                            btn,
                            if is_metadata_loading { None } else { Some("Refresh metadata now") },
                            TooltipPosition::Top,
                        )
                    };

                    let last_updated = {
                        let label = if is_metadata_loading {
                            "Refreshing metadata…".to_string()
                        } else {
                            match last_metadata_update {
                                Some(at) => {
                                    format!(
                                        "Metadata last update: {}",
                                        data::util::relative_time_label(at)
                                    )
                                }
                                None => "Metadata not fetched yet".to_string(),
                            }
                        };

                        text(label).size(11)
                    };

                    column![
                        text("Startup Metadata Caching").size(13),
                        text("Persist parsed instrument rules to disk to start without API roundtrips").size(11),
                        row![checkbox, refresh_btn].align_y(Alignment::Center).spacing(8),
                        last_updated,
                    ]
                    .spacing(6)
                }
            };

            column![
                container(display_unit_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(fetch_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(metadata_cache_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
                container(feeds_info)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .into()
        }
        GlobalSettingsCategory::System => {
            let open_data_folder = {
                let btn = button(text("Open Data Folder").size(12))
                    .padding([6, 12])
                    .on_press(Message::DataFolderRequested);

                tooltip(
                    btn,
                    Some("Open local directory where configs, database, and cache files reside"),
                    TooltipPosition::Top,
                )
            };

            let storage_section = column![
                text("Storage & Data Directory").size(13),
                text(
                    "Location of layout templates, SQLite trade journal, and local footprint cache"
                )
                .size(11),
                open_data_folder,
            ]
            .spacing(6);

            #[cfg(feature = "volume-search")]
            let vol_search_section = {
                let checkbox = checkbox(show_volume_search)
                    .label("Show Volume Search in sidebar")
                    .on_toggle(Message::ToggleVolumeSearchSidebar);

                let item = tooltip(
                    checkbox,
                    Some("Display Volume Search (Пошук об'єму) tool in sidebar navigation"),
                    TooltipPosition::Top,
                );

                column![
                    text("Sidebar Tools").size(13),
                    text("Enable or disable specialized analytical tool shortcuts in the navigation dock").size(11),
                    item,
                ]
                .spacing(6)
            };

            let sys_info = column![
                text("Application Information").size(13),
                column![
                    text("• Hawk Terminal: v0.9.4 Native Desktop").size(11),
                    text("• Cache Engine: LZ4 binary compressed footprint trade cache").size(11),
                    text("• UI Framework: Iced with GPU-accelerated canvas renderer").size(11),
                ]
                .spacing(3)
                .padding(padding::left(6)),
            ]
            .spacing(6);

            let mut col = column![
                container(storage_section)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
            ]
            .spacing(10);

            #[cfg(feature = "volume-search")]
            {
                col = col.push(
                    container(vol_search_section)
                        .style(style::modal_container)
                        .padding(8)
                        .width(Length::Fill),
                );
            }

            col = col.push(
                container(sys_info)
                    .style(style::modal_container)
                    .padding(8)
                    .width(Length::Fill),
            );

            col.into()
        }
    };

    let right_col = column![
        right_header,
        rule::horizontal(1.0).style(style::split_ruler),
        container(scrollable_content(category_content))
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .spacing(8)
    .width(Length::Fill)
    .height(Length::Fill);

    let right_area = container(right_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(padding::left(10));

    let layout_row = row![
        sidebar,
        rule::vertical(1.0).style(style::split_ruler),
        right_area,
    ]
    .height(Length::Fill)
    .spacing(0);

    container(layout_row)
        .width(Length::Fixed(580.0))
        .height(Length::Fill)
        .padding(14)
        .style(style::dashboard_modal)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_settings_categories_metadata() {
        for cat in GlobalSettingsCategory::ALL {
            assert!(!cat.title().is_empty());
        }
        assert_eq!(
            GlobalSettingsCategory::default(),
            GlobalSettingsCategory::General
        );
        assert_eq!(GlobalSettingsCategory::ALL.len(), 4);
    }
}
