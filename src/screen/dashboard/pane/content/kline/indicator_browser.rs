use crate::chart::kline::KlineChart;
use crate::screen::dashboard::pane::{Event, Message as PaneMessage};
use crate::style::{self, Icon, icon_text};
use crate::tooltip;
use data::chart::indicator::{Indicator, KlineIndicator};
use data::chart::kline::indicators::{
    IndicatorId, IndicatorInputSource, all_indicators, declared_input_requirements,
};
use exchange::adapter::MarketKind;
use iced::{
    Alignment, Element, Length,
    widget::{
        button, column, container, pane_grid, row, scrollable, space, text, text_input,
        tooltip::Position as TooltipPosition,
    },
};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    SearchChanged(String),
    ToggleIndicator(IndicatorId),
    ToggleBuiltin(KlineIndicator),
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub search_query: String,
    pub search_box_id: iced::widget::Id,
    pub scroll_id: iced::widget::Id,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            search_box_id: iced::widget::Id::unique(),
            scroll_id: iced::widget::Id::unique(),
        }
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
    }

    pub fn search_box_id(&self) -> iced::widget::Id {
        self.search_box_id.clone()
    }

    pub fn scroll_id(&self) -> iced::widget::Id {
        self.scroll_id.clone()
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn matches_query(&self, label: &str) -> bool {
        let query = self.search_query.trim();
        query.is_empty() || label.to_lowercase().contains(&query.to_lowercase())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorInputGroup {
    Candles,
    CandlesOrTrades,
    CandlesAndTrades,
    Trades,
    OpenInterest,
    CandlesAndOpenInterest,
    CandlesOrTradesAndOpenInterest,
    TradesAndOpenInterest,
    CandlesTradesAndOpenInterest,
    NoExternalData,
}

impl IndicatorInputGroup {
    pub const ALL: [Self; 10] = [
        Self::Candles,
        Self::CandlesOrTrades,
        Self::CandlesAndTrades,
        Self::Trades,
        Self::OpenInterest,
        Self::CandlesAndOpenInterest,
        Self::CandlesOrTradesAndOpenInterest,
        Self::TradesAndOpenInterest,
        Self::CandlesTradesAndOpenInterest,
        Self::NoExternalData,
    ];

    pub fn for_indicator(indicator: &IndicatorId) -> Self {
        let requirements = declared_input_requirements(indicator);
        let requires = |source| {
            requirements
                .iter()
                .any(|requirement| requirement.source == source && requirement.is_required())
        };

        let requires_directional_volume = requires(IndicatorInputSource::DirectionalVolume);
        if requires_directional_volume {
            return if requires(IndicatorInputSource::OpenInterest) {
                Self::CandlesOrTradesAndOpenInterest
            } else {
                Self::CandlesOrTrades
            };
        }

        match (
            requires(IndicatorInputSource::Klines),
            requires(IndicatorInputSource::Trades),
            requires(IndicatorInputSource::OpenInterest),
        ) {
            (true, false, false) => Self::Candles,
            (true, true, false) => Self::CandlesAndTrades,
            (false, true, false) => Self::Trades,
            (false, false, true) => Self::OpenInterest,
            (true, false, true) => Self::CandlesAndOpenInterest,
            (false, true, true) => Self::TradesAndOpenInterest,
            (true, true, true) => Self::CandlesTradesAndOpenInterest,
            (false, false, false) => Self::NoExternalData,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Candles => "Candles",
            Self::CandlesOrTrades => "Candles or trades",
            Self::CandlesAndTrades => "Candles + trades",
            Self::Trades => "Trades",
            Self::OpenInterest => "Open interest",
            Self::CandlesAndOpenInterest => "Candles + open interest",
            Self::CandlesOrTradesAndOpenInterest => "Candles or trades + open interest",
            Self::TradesAndOpenInterest => "Trades + open interest",
            Self::CandlesTradesAndOpenInterest => "Candles + trades + open interest",
            Self::NoExternalData => "No external data",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableIndicator {
    pub indicator: IndicatorId,
    pub name: String,
    pub unavailable_reason: Option<&'static str>,
    pub active_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvailableBuiltin {
    pub indicator: KlineIndicator,
    pub active: bool,
    pub available: bool,
}

pub fn view<'a>(
    pane: pane_grid::Pane,
    chart: &'a KlineChart,
    builtin_selected: &[KlineIndicator],
    search_query: &str,
    market: Option<MarketKind>,
) -> Element<'a, PaneMessage> {
    let mut state = State::new();
    state.set_search_query(search_query.to_string());

    // 1. Built-in indicators
    let available_builtin: Vec<AvailableBuiltin> = market
        .map(KlineIndicator::for_market)
        .unwrap_or(&[])
        .iter()
        .map(|&ind| AvailableBuiltin {
            indicator: ind,
            active: builtin_selected.contains(&ind),
            available: true,
        })
        .filter(|b| state.matches_query(&b.indicator.to_string()))
        .collect();

    // 2. Scripted indicators from catalog
    let available_scripted: Vec<AvailableIndicator> = all_indicators()
        .into_iter()
        .map(|indicator| {
            let name = indicator.definition().display_name.clone();
            let unavailable_reason = if let Some(m) = market {
                if !indicator.supports_market(m) {
                    Some("Not supported for this market type")
                } else {
                    None
                }
            } else {
                None
            };

            let active_count = chart
                .indicator_instances
                .instances()
                .iter()
                .filter(|inst| inst.kind == indicator)
                .count();

            AvailableIndicator {
                indicator,
                name,
                unavailable_reason,
                active_count,
            }
        })
        .filter(|a| state.matches_query(&a.name) || state.matches_query(a.indicator.as_str()))
        .collect();

    let search_bar = row![
        text_input("Search indicators...", state.search_query())
            .style(|theme, status| style::validated_text_input(theme, status, true))
            .on_input(move |q| PaneMessage::PaneEvent(pane, Event::IndicatorSearchChanged(q)))
            .id(state.search_box_id())
            .padding(6)
            .width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    let mut content_sections = iced::widget::Column::new().spacing(10);

    // Section 1: Builtin
    if !available_builtin.is_empty() {
        let mut builtin_col = iced::widget::Column::new().spacing(2);
        for b in available_builtin {
            let row_btn = builtin_row(pane, b);
            builtin_col = builtin_col.push(row_btn);
        }

        content_sections = content_sections.push(
            column![
                text("Built-in Indicators").size(12),
                builtin_col.width(Length::Fill),
            ]
            .spacing(4)
            .width(Length::Fill),
        );
    }

    // Section 2: Scripted grouped
    let mut grouped: Vec<(IndicatorInputGroup, Vec<AvailableIndicator>)> = IndicatorInputGroup::ALL
        .into_iter()
        .map(|group| (group, Vec::new()))
        .collect();

    for ind in available_scripted {
        let group = IndicatorInputGroup::for_indicator(&ind.indicator);
        if let Some((_, entries)) = grouped.iter_mut().find(|(c, _)| *c == group) {
            entries.push(ind);
        }
    }

    for (group, indicators) in grouped {
        if indicators.is_empty() {
            continue;
        }

        let mut group_col = iced::widget::Column::new().spacing(2);
        for ind in indicators {
            let row_btn = scripted_row(pane, ind);
            group_col = group_col.push(row_btn);
        }

        content_sections = content_sections.push(
            column![text(group.label()).size(12), group_col.width(Length::Fill),]
                .spacing(4)
                .width(Length::Fill),
        );
    }

    let scroll = scrollable::Scrollable::with_direction(
        content_sections,
        scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4)),
    )
    .id(state.scroll_id())
    .height(Length::Fixed(360.0))
    .style(style::scroll_bar);

    container(column![search_bar, scroll].spacing(10))
        .width(Length::Fixed(320.0))
        .padding(14)
        .style(style::chart_modal)
        .into()
}

fn builtin_row<'a>(pane: pane_grid::Pane, b: AvailableBuiltin) -> Element<'a, PaneMessage> {
    let mut content = row![text(b.indicator.to_string()).size(13), space::horizontal(),];

    if b.active {
        content = content.push(container(icon_text(Icon::Checkmark, 12)));
    }

    button(content.align_y(Alignment::Center).width(Length::Fill))
        .width(Length::Fill)
        .style(move |theme, status| style::button::modifier(theme, status, b.active))
        .on_press(PaneMessage::PaneEvent(
            pane,
            Event::ToggleIndicator(b.indicator.into()),
        ))
        .into()
}

fn scripted_row<'a>(pane: pane_grid::Pane, ind: AvailableIndicator) -> Element<'a, PaneMessage> {
    let is_active = ind.active_count > 0;
    let mut content = row![text(ind.name).size(13), space::horizontal(),];

    if ind.active_count > 0 {
        let badge_text = if ind.active_count > 1 {
            format!("x{}", ind.active_count)
        } else {
            String::new()
        };
        let mut right = row![].spacing(4).align_y(Alignment::Center);
        if !badge_text.is_empty() {
            right = right.push(text(badge_text).size(11));
        }
        right = right.push(container(icon_text(Icon::Checkmark, 12)));
        content = content.push(right);
    }

    let is_available = ind.unavailable_reason.is_none();
    let btn = button(content.align_y(Alignment::Center).width(Length::Fill))
        .width(Length::Fill)
        .style(move |theme, status| style::button::modifier(theme, status, is_active));

    if is_available {
        let btn = btn.on_press(PaneMessage::PaneEvent(
            pane,
            Event::ToggleScriptedIndicator(ind.indicator),
        ));
        btn.into()
    } else {
        tooltip(btn, ind.unavailable_reason, TooltipPosition::Top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indicator_browser_state_query_filtering() {
        let mut state = State::new();
        assert!(state.matches_query("Relative Strength Index"));

        state.set_search_query("RSI".to_string());
        assert!(state.matches_query("core.rsi"));
        assert!(state.matches_query("RSI"));
        assert!(!state.matches_query("Bollinger Bands"));
    }

    #[test]
    fn test_indicator_input_groups_exhaustiveness() {
        assert_eq!(IndicatorInputGroup::ALL.len(), 10);
        for group in IndicatorInputGroup::ALL {
            assert!(!group.label().is_empty());
        }
    }

    #[test]
    fn test_input_group_for_core_indicators() {
        let _ = data::chart::kline::indicators::loader::initialize_indicator_catalog();
        let rsi_id = IndicatorId::parse("core.rsi").unwrap();
        let group = IndicatorInputGroup::for_indicator(&rsi_id);
        assert_eq!(group, IndicatorInputGroup::Candles);

        let cvd_id = IndicatorId::parse("core.cumulative-volume-delta").unwrap();
        let group_cvd = IndicatorInputGroup::for_indicator(&cvd_id);
        assert!(matches!(
            group_cvd,
            IndicatorInputGroup::CandlesOrTrades
                | IndicatorInputGroup::CandlesAndTrades
                | IndicatorInputGroup::Trades
                | IndicatorInputGroup::Candles
        ));
    }
}
