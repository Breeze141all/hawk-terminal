use exchange::{
    Ticker, TickerStats,
    adapter::{Exchange, ExchangeInclusive, MarketKind},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub favorited_tickers: Vec<Ticker>,
    pub show_favorites: bool,
    pub selected_sort_option: SortOptions,
    pub selected_exchanges: Vec<ExchangeInclusive>,
    pub selected_markets: Vec<MarketKind>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            favorited_tickers: vec![],
            show_favorites: false,
            selected_sort_option: SortOptions::VolumeDesc,
            selected_exchanges: ExchangeInclusive::ALL.to_vec(),
            selected_markets: MarketKind::ALL.into_iter().collect(),
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub enum SortOptions {
    #[default]
    VolumeAsc,
    VolumeDesc,
    ChangeAsc,
    ChangeDesc,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PriceChangeDirection {
    Increased,
    Decreased,
    Unchanged,
}

#[derive(Clone, Copy)]
pub struct TickerRowData {
    pub exchange: Exchange,
    pub ticker: Ticker,
    pub stats: TickerStats,
    pub previous_stats: Option<TickerStats>,
    pub is_favorited: bool,
}

#[derive(Clone)]
pub struct TickerDisplayData {
    pub display_ticker: String,
    pub daily_change_pct: String,
    pub volume_display: String,
    pub mark_price_display: String,
    pub price_unchanged_part: String,
    pub price_changed_part: String,
    pub price_change_direction: PriceChangeDirection,
    pub card_color_alpha: f32,
}

pub fn compute_display_data(
    ticker: &Ticker,
    stats: &TickerStats,
    previous_price: Option<f32>,
) -> TickerDisplayData {
    let (display_ticker, _market) = ticker.display_symbol_and_type();

    let current_price = stats.mark_price;
    let (price_unchanged_part, price_changed_part, price_change_direction) =
        if let Some(prev_price) = previous_price {
            split_price_changes(prev_price, current_price)
        } else {
            (
                current_price.to_string(),
                String::new(),
                PriceChangeDirection::Unchanged,
            )
        };

    TickerDisplayData {
        display_ticker,
        daily_change_pct: super::util::pct_change(stats.daily_price_chg),
        volume_display: super::util::currency_abbr(stats.daily_volume),
        mark_price_display: stats.mark_price.to_string(),
        price_unchanged_part,
        price_changed_part,
        price_change_direction,
        card_color_alpha: { (stats.daily_price_chg / 8.0).clamp(-1.0, 1.0) },
    }
}

/// Compute common prefix byte boundary safe across all UTF-8 characters.
pub fn split_price_str<'a>(prev_str: &'a str, curr_str: &'a str) -> (&'a str, &'a str) {
    let mut split_byte_idx = 0;
    for ((byte_idx, c1), (_, c2)) in curr_str.char_indices().zip(prev_str.char_indices()) {
        if c1 != c2 {
            break;
        }
        split_byte_idx = byte_idx + c1.len_utf8();
    }
    (&curr_str[..split_byte_idx], &curr_str[split_byte_idx..])
}

pub fn split_price_changes(
    previous_price: f32,
    current_price: f32,
) -> (String, String, PriceChangeDirection) {
    if previous_price == current_price {
        return (
            current_price.to_string(),
            String::new(),
            PriceChangeDirection::Unchanged,
        );
    }

    let prev_str = previous_price.to_string();
    let curr_str = current_price.to_string();

    let direction = if current_price > previous_price {
        PriceChangeDirection::Increased
    } else {
        PriceChangeDirection::Decreased
    };

    let (unchanged_part, changed_part) = split_price_str(&prev_str, &curr_str);
    (
        unchanged_part.to_string(),
        changed_part.to_string(),
        direction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dat08_utf8_multibyte_split_safe() {
        // Multi-byte Unicode currency symbols
        let (unchanged, changed) = split_price_str("€123.45", "€123.89");
        assert_eq!(unchanged, "€123.");
        assert_eq!(changed, "89");

        let (unchanged, changed) = split_price_str("¥1000", "¥1050");
        assert_eq!(unchanged, "¥10");
        assert_eq!(changed, "50");

        let (unchanged, changed) = split_price_str("₴41.50", "₴41.75");
        assert_eq!(unchanged, "₴41.");
        assert_eq!(changed, "75");

        // Unicode minus sign (\u{2212}, 3 bytes)
        let (unchanged, changed) = split_price_str("−10.5", "−10.8");
        assert_eq!(unchanged, "−10.");
        assert_eq!(changed, "8");

        // Differing lengths with mismatch at character 0
        let (unchanged, changed) = split_price_str("20.0", "100.0");
        assert_eq!(unchanged, "");
        assert_eq!(changed, "100.0");

        // Differing lengths with common prefix
        let (unchanged, changed) = split_price_str("10", "10.5");
        assert_eq!(unchanged, "10");
        assert_eq!(changed, ".5");

        let (unchanged, changed) = split_price_str("10.5", "10");
        assert_eq!(unchanged, "10");
        assert_eq!(changed, "");

        // Identical and empty strings
        let (unchanged, changed) = split_price_str("123.45", "123.45");
        assert_eq!(unchanged, "123.45");
        assert_eq!(changed, "");

        let (unchanged, changed) = split_price_str("", "100.0");
        assert_eq!(unchanged, "");
        assert_eq!(changed, "100.0");

        // Float-based split_price_changes integration test
        let (unchanged, changed, dir) = split_price_changes(123.45, 123.89);
        assert_eq!(dir, PriceChangeDirection::Increased);
        assert_eq!(unchanged, "123.");
        assert_eq!(changed, "89");
    }
}
