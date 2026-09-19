use super::ScaleFactor;
use super::sidebar::Sidebar;
use super::timezone::UserTimezone;
use crate::journal::JournalMode;
use crate::layout::WindowSpec;
use crate::{AudioStream, Layout, Theme};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Layouts {
    pub layouts: Vec<Layout>,
    pub active_layout: Option<String>,
}

pub const DEFAULT_HAWK_STATE_JSON: &str = include_str!("../default_state.json");

pub fn default_state() -> State {
    serde_json::from_str(DEFAULT_HAWK_STATE_JSON)
        .expect("Embedded default_state.json must be valid")
}

pub fn default_hawk_layout() -> Layout {
    let state = default_state();
    state
        .layout_manager
        .layouts
        .into_iter()
        .next()
        .expect("Default state must contain at least one layout")
}

#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub layout_manager: Layouts,
    pub selected_theme: Theme,
    pub custom_theme: Option<Theme>,
    pub main_window: Option<WindowSpec>,
    pub timezone: UserTimezone,
    pub sidebar: Sidebar,
    pub scale_factor: ScaleFactor,
    pub audio_cfg: AudioStream,
    pub trade_fetch_enabled: bool,
    pub size_in_quote_ccy: exchange::SizeUnit,
    pub journal_mode: JournalMode,
    #[serde(default)]
    pub default_kline_config: Option<crate::chart::kline::Config>,
}

impl State {
    pub fn from_parts(
        layout_manager: Layouts,
        selected_theme: Theme,
        custom_theme: Option<Theme>,
        main_window: Option<WindowSpec>,
        timezone: UserTimezone,
        sidebar: Sidebar,
        scale_factor: ScaleFactor,
        audio_cfg: AudioStream,
        volume_size_unit: exchange::SizeUnit,
        journal_mode: JournalMode,
        default_kline_config: Option<crate::chart::kline::Config>,
    ) -> Self {
        State {
            layout_manager,
            selected_theme: Theme(selected_theme.0),
            custom_theme: custom_theme.map(|t| Theme(t.0)),
            main_window,
            timezone,
            sidebar,
            scale_factor,
            audio_cfg,
            trade_fetch_enabled: exchange::fetcher::is_trade_fetch_enabled(),
            size_in_quote_ccy: volume_size_unit,
            journal_mode,
            default_kline_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_contains_hawk_template() {
        let state = default_state();
        assert_eq!(state.layout_manager.active_layout.as_deref(), Some("Hawk"));
        assert!(!state.layout_manager.layouts.is_empty());
        assert_eq!(state.layout_manager.layouts[0].name, "Hawk");

        let hawk_layout = default_hawk_layout();
        assert_eq!(hawk_layout.name, "Hawk");
    }
}
