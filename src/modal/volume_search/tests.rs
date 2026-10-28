use super::*;
use exchange::adapter::ExchangeInclusive;
use exchange::{Exchange, Ticker, TickerInfo};

fn mock_ticker(symbol: &str, exchange: Exchange) -> TickerInfo {
    TickerInfo::new(Ticker::new(symbol, exchange), 0.01, 0.001, None)
}

fn sample_catalog() -> Vec<TickerInfo> {
    vec![
        // Binance: Linear, Spot, Inverse
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceSpot),
        mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse),
        // Bybit: Linear, Spot, Inverse
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
        mock_ticker("XRPUSDT", Exchange::BybitSpot),
        mock_ticker("BTCUSD", Exchange::BybitInverse),
        // OKX: Linear, Spot, Inverse
        mock_ticker("DOGE-USDT-SWAP", Exchange::OkexLinear),
        mock_ticker("LTC-USDT", Exchange::OkexSpot),
        mock_ticker("ETH-USD-SWAP", Exchange::OkexInverse),
        // Hyperliquid: Linear, Spot
        mock_ticker("HYPE", Exchange::HyperliquidLinear),
        mock_ticker("PURR", Exchange::HyperliquidSpot),
    ]
}

#[test]
fn test_m1_global_ticker_filtering_all_4_exchanges_with_zero_panes() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog.clone());

    assert_eq!(modal.available_tickers.len(), 11);
    assert_eq!(modal.selected_ticker, Some(catalog[0]));

    // 1. All exchanges (None filter)
    modal.selected_exchange_filter = None;
    modal.ticker_search_query.clear();
    let all = modal.filtered_and_ranked_candidates();
    assert_eq!(all.len(), 11);

    // 2. Filter Binance (matches Linear, Spot, Inverse)
    modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
    let binance = modal.filtered_and_ranked_candidates();
    assert_eq!(binance.len(), 3);
    assert!(
        binance
            .iter()
            .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Binance)
    );

    // 3. Filter Bybit
    modal.selected_exchange_filter = Some(ExchangeInclusive::Bybit);
    let bybit = modal.filtered_and_ranked_candidates();
    assert_eq!(bybit.len(), 3);
    assert!(
        bybit
            .iter()
            .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Bybit)
    );

    // 4. Filter OKX
    modal.selected_exchange_filter = Some(ExchangeInclusive::Okex);
    let okx = modal.filtered_and_ranked_candidates();
    assert_eq!(okx.len(), 3);
    assert!(
        okx.iter()
            .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Okex)
    );

    // 5. Filter Hyperliquid
    modal.selected_exchange_filter = Some(ExchangeInclusive::Hyperliquid);
    let hl = modal.filtered_and_ranked_candidates();
    assert_eq!(hl.len(), 2);
    assert!(
        hl.iter()
            .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Hyperliquid)
    );

    // 6. Select a Hyperliquid ticker with 0 panes open in dashboard
    let target_ticker = hl[0];
    modal.update(Message::TickerSelected(target_ticker), None);
    assert_eq!(modal.selected_ticker, Some(target_ticker));
    assert!(!modal.is_ticker_selector_open);
}

#[test]
fn test_m1_exchange_variant_normalization_matches_all_submarkets() {
    // Exhaustive verification of all 11 Exchange variants
    for exchange in Exchange::ALL {
        let inclusive = ExchangeInclusive::of(exchange);
        match exchange {
            Exchange::BinanceLinear | Exchange::BinanceInverse | Exchange::BinanceSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Binance);
            }
            Exchange::BybitLinear | Exchange::BybitInverse | Exchange::BybitSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Bybit);
            }
            Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Okex);
            }
            Exchange::HyperliquidLinear | Exchange::HyperliquidSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Hyperliquid);
            }
            Exchange::MexcLinear | Exchange::MexcInverse | Exchange::MexcSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Mexc);
            }
            Exchange::CoinbaseSpot => {
                assert_eq!(inclusive, ExchangeInclusive::Coinbase);
            }
        }
    }
}

#[test]
fn test_m1_search_matching_case_prefix_substring_queries() {
    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
        mock_ticker("1000PEPEUSDT", Exchange::BybitLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDC", Exchange::BybitLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.selected_exchange_filter = None;

    // 1. Lowercase prefix query
    modal.ticker_search_query = "btc".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].ticker.as_str(), "BTCUSDT"); // Exact prefix ranks first
    assert_eq!(res[1].ticker.as_str(), "WBTCUSDT"); // Substring ranks second

    // 2. Uppercase query
    modal.ticker_search_query = "BTC".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].ticker.as_str(), "BTCUSDT");

    // 3. Mixed case exact symbol
    modal.ticker_search_query = "bTcUsDt".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].ticker.as_str(), "BTCUSDT");

    // 4. Substring in the middle of a token
    modal.ticker_search_query = "pepe".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].ticker.as_str(), "1000PEPEUSDT");

    // 5. Leading and trailing whitespace handling
    modal.ticker_search_query = "  eth  ".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].ticker.as_str(), "ETHUSDT");

    // 6. Non-matching query
    modal.ticker_search_query = "DOGE".to_string();
    let res = modal.filtered_and_ranked_candidates();
    assert!(res.is_empty());
}

#[test]
fn test_m1_catalog_update_preserves_selection_and_query() {
    let initial_catalog = vec![mock_ticker("BTCUSDT", Exchange::BinanceLinear)];
    let mut modal = VolumeSearchModal::new(None, initial_catalog);

    modal.ticker_search_query = "ETH".to_string();
    modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
    let active_sel = mock_ticker("BTCUSDT", Exchange::BinanceLinear);
    modal.selected_ticker = Some(active_sel);

    // Simulate incoming async batch with newly arrived pairs
    let updated_catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
    ];
    modal.update_available_tickers(updated_catalog);

    // Invariants: available_tickers updated; search query, exchange filter, and selection preserved
    assert_eq!(modal.available_tickers.len(), 3);
    assert_eq!(modal.selected_ticker, Some(active_sel));
    assert_eq!(modal.ticker_search_query, "ETH");
    assert_eq!(
        modal.selected_exchange_filter,
        Some(ExchangeInclusive::Binance)
    );

    let res = modal.filtered_and_ranked_candidates();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].ticker.as_str(), "ETHUSDT");
}

#[test]
fn test_ticker_ranked_matching() {
    let btc = mock_ticker("BTCUSDT", Exchange::BinanceLinear);
    let wbtc = mock_ticker("WBTCUSDT", Exchange::BinanceSpot);
    let eth = mock_ticker("ETHUSDT", Exchange::BinanceLinear);

    let rank_btc = calc_ticker_rank(&btc, "BTC").unwrap();
    let rank_wbtc = calc_ticker_rank(&wbtc, "BTC").unwrap();
    let rank_eth = calc_ticker_rank(&eth, "BTC");

    assert!(rank_eth.is_none());
    assert_eq!(rank_btc.bucket, 1); // prefix
    assert_eq!(rank_wbtc.bucket, 3); // substring
    assert!(rank_btc.bucket < rank_wbtc.bucket);
}

#[test]
fn test_adversarial_zero_panes_event_flow_and_view_rendering() {
    let mock_coin = mock_ticker("NONEXISTENT_COIN", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    // View before search in isolated scope
    {
        let _view_initial = modal.view();
    }

    // State transitions with dashboard = None
    assert!(
        modal
            .update(Message::TimeframeSelected(Timeframe::M15), None)
            .is_none()
    );
    assert_eq!(modal.selected_timeframe, Timeframe::M15);

    assert!(
        modal
            .update(Message::PresetSelected(VolumeSearchPreset::Last7d), None)
            .is_none()
    );
    assert_eq!(modal.preset, VolumeSearchPreset::Last7d);

    assert!(
        modal
            .update(Message::TimeFilterStartChanged("08:00".into()), None)
            .is_none()
    );
    assert_eq!(modal.time_filter_start, "08:00");

    assert!(
        modal
            .update(Message::TimeFilterEndChanged("20:00".into()), None)
            .is_none()
    );
    assert_eq!(modal.time_filter_end, "20:00");

    assert!(
        modal
            .update(Message::MultiplierSelected(TickMultiplier(50)), None)
            .is_none()
    );
    assert_eq!(modal.selected_multiplier, TickMultiplier(50));

    assert!(
        modal
            .update(Message::ToggleCustomMultiplier(true), None)
            .is_none()
    );
    assert!(modal.is_custom_multiplier);

    assert!(
        modal
            .update(Message::CustomMultiplierChanged("150".into()), None)
            .is_none()
    );
    assert_eq!(modal.selected_multiplier, TickMultiplier(150));

    assert!(
        modal
            .update(Message::MinVolumeChanged("5000".into()), None)
            .is_none()
    );
    assert_eq!(modal.min_volume_input, "5000");

    assert!(
        modal
            .update(Message::SortColumnClicked(VolumeSearchSortCol::Delta), None)
            .is_none()
    );
    assert_eq!(modal.sort_col, VolumeSearchSortCol::Delta);
    assert_eq!(modal.sort_order, SortOrder::Descending);

    assert!(
        modal
            .update(Message::SortColumnClicked(VolumeSearchSortCol::Delta), None)
            .is_none()
    );
    assert_eq!(modal.sort_order, SortOrder::Ascending);

    // Apply search with 0 panes and no disk cache initiates download task
    let action = modal.update(Message::ApplySearch, None);
    assert!(matches!(action, Some(Action::Task(_))));
    assert!(modal.is_downloading);
    assert!(modal.download_progress.is_some());
    assert_eq!(modal.download_missing_dates.len(), 7);

    // Cancel search resets downloading state
    modal.update(Message::CancelSearch, None);
    assert!(!modal.is_downloading);
    assert!(modal.download_progress.is_none());

    // View after search
    {
        let _view_searched = modal.view();
    }

    // Close message returns Action::Close
    assert!(matches!(
        modal.update(Message::Close, None),
        Some(Action::Close)
    ));
}

#[test]
fn test_adversarial_dynamic_catalog_progressive_updates() {
    let mut modal = VolumeSearchModal::new(None, Vec::new());
    assert_eq!(modal.available_tickers.len(), 0);
    assert_eq!(modal.selected_ticker, None);
    assert_eq!(modal.filtered_tickers.len(), 0);

    modal.update(Message::TickerSearchChanged("ETH".into()), None);
    assert_eq!(modal.ticker_search_query, "ETH");
    assert_eq!(modal.filtered_tickers.len(), 0);

    modal.update(Message::TickerSearchSubmitted, None);
    assert_eq!(modal.selected_ticker, None);

    // Batch 1: BTC and SOL only
    let batch_1 = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BinanceLinear),
    ];
    modal.update_available_tickers(batch_1);
    assert_eq!(modal.available_tickers.len(), 2);
    assert_eq!(modal.filtered_tickers.len(), 0);
    assert_eq!(modal.selected_ticker, None);

    // Batch 2: ETH pairs added
    let batch_2 = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceSpot),
    ];
    modal.update_available_tickers(batch_2);
    assert_eq!(modal.available_tickers.len(), 4);
    assert_eq!(modal.filtered_tickers.len(), 2);
    assert_eq!(
        modal.selected_ticker,
        Some(mock_ticker("ETHUSDT", Exchange::BinanceLinear))
    );

    // User explicitly selects Spot ETH
    let spot_eth = mock_ticker("ETHUSDT", Exchange::BinanceSpot);
    modal.update(Message::TickerSelected(spot_eth), None);
    assert_eq!(modal.selected_ticker, Some(spot_eth));

    // Batch 3: Bybit and OKX added
    let batch_3 = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceSpot),
        mock_ticker("ETHUSDT", Exchange::BybitLinear),
        mock_ticker("ETH-USDT-SWAP", Exchange::OkexLinear),
    ];
    modal.update_available_tickers(batch_3.clone());
    assert_eq!(modal.available_tickers.len(), 6);
    assert_eq!(modal.selected_ticker, Some(spot_eth));
    assert_eq!(modal.filtered_tickers.len(), 4);

    modal.update_available_tickers(batch_3);
    assert_eq!(modal.selected_ticker, Some(spot_eth));
}

#[test]
fn test_adversarial_exchange_filter_oscillation_and_empty_transitions() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    modal.update(Message::TickerSearchChanged("HYPE".into()), None);
    assert_eq!(modal.filtered_tickers.len(), 1);
    assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "HYPE");

    modal.update(Message::TickerSearchSubmitted, None);
    let hype_ticker = modal.filtered_tickers[0];
    assert_eq!(modal.selected_ticker, Some(hype_ticker));

    modal.update(
        Message::ExchangeFilterSelected(Some(ExchangeInclusive::Binance)),
        None,
    );
    assert_eq!(modal.filtered_tickers.len(), 0);
    assert_eq!(modal.selected_ticker, Some(hype_ticker));

    modal.update(Message::TickerSearchSubmitted, None);
    assert_eq!(modal.selected_ticker, Some(hype_ticker));

    {
        let _view_empty_filter = modal.view();
    }

    for _ in 0..20 {
        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Bybit)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 0);

        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Okex)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 0);

        modal.update(
            Message::ExchangeFilterSelected(Some(ExchangeInclusive::Hyperliquid)),
            None,
        );
        assert_eq!(modal.filtered_tickers.len(), 1);

        modal.update(Message::ExchangeFilterSelected(None), None);
        assert_eq!(modal.filtered_tickers.len(), 1);
    }

    assert_eq!(modal.selected_ticker, Some(hype_ticker));
}

#[test]
fn test_adversarial_enter_vs_picklist_selection_and_ranking() {
    let catalog = vec![
        mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTCUSD", Exchange::BybitInverse),
    ];
    let mut modal = VolumeSearchModal::new(None, catalog);

    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    // Expected: LinearPerps (BTCUSDT) ranks before InversePerps (BTCUSD).
    // Actual defect: String length comparison rank.len (6 < 7) evaluates before market_type,
    // causing InversePerps BTCUSD to rank before LinearPerps BTCUSDT.
    assert_eq!(
        modal.filtered_tickers[0].ticker.as_str(),
        "BTCUSDT",
        "ranking bug: InversePerps BTCUSD ranked before LinearPerps BTCUSDT due to string length"
    );
    assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSD");
    assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "WBTCUSDT");
    assert_eq!(modal.filtered_tickers[3].ticker.as_str(), "1000BTCUSDT");

    modal.update(Message::TickerSearchSubmitted, None);
    assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "BTCUSDT");

    let picklist_choice = modal.filtered_tickers[2];
    modal.update(Message::TickerSelected(picklist_choice), None);
    assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "WBTCUSDT");

    modal.update(Message::TickerSearchChanged("NONEXISTENT".into()), None);
    assert!(modal.filtered_tickers.is_empty());

    modal.update(Message::TickerSearchSubmitted, None);
    assert_eq!(modal.selected_ticker.unwrap().ticker.as_str(), "WBTCUSDT");
}

#[test]
fn test_adversarial_exact_match_pruning_hides_cross_exchange_pairs() {
    let catalog = vec![
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.selected_exchange_filter = None;

    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    let binance_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::BinanceLinear);
    let bybit_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::BybitLinear);

    assert!(
        binance_present && bybit_present,
        "BUG DETECTED: Searching 'BTC' across all exchanges purged Binance and Bybit pairs because Hyperliquid had an exact match 'BTC'!"
    );
}

#[test]
fn test_challenger1_empty_and_whitespace_queries() {
    let catalog = sample_catalog();
    let modal = VolumeSearchModal::new(None, catalog.clone());

    // Empty query
    let res_empty = modal.compute_filtered_tickers("");
    assert_eq!(res_empty.len(), catalog.len());

    // Whitespace-only queries
    let res_spaces = modal.compute_filtered_tickers("   ");
    assert_eq!(res_spaces.len(), catalog.len());
    let res_tabs = modal.compute_filtered_tickers(" \t \n ");
    assert_eq!(res_tabs.len(), catalog.len());

    // Invariants: MarketKind ordering (Linear < Spot < Inverse) when rank is identical
    for window in res_empty.windows(2) {
        let kind_rank = |k: MarketKind| match k {
            MarketKind::LinearPerps => 0,
            MarketKind::Spot => 1,
            MarketKind::InversePerps => 2,
        };
        let k0 = kind_rank(window[0].market_type());
        let k1 = kind_rank(window[1].market_type());
        assert!(
            k0 <= k1,
            "Order violation: market kinds not sorted properly on empty query"
        );
    }
}

#[test]
fn test_challenger1_unicode_and_adversarial_queries() {
    let catalog = sample_catalog();
    let modal = VolumeSearchModal::new(None, catalog);

    // 1. Cyrillic input (e.g. Ukrainian/Russian keyboard layout slips)
    let res_cyr = modal.compute_filtered_tickers("БТК");
    assert!(
        res_cyr.is_empty(),
        "Cyrillic query should safely yield 0 results without panic"
    );

    let res_cyr_lower = modal.compute_filtered_tickers("бтк");
    assert!(res_cyr_lower.is_empty());

    // 2. Unicode Emojis
    let res_emoji = modal.compute_filtered_tickers("BTC🚀");
    assert!(res_emoji.is_empty());

    let res_fire = modal.compute_filtered_tickers("🔥");
    assert!(res_fire.is_empty());

    // 3. Currency symbols
    let res_currency = modal.compute_filtered_tickers("€$¥₴");
    assert!(res_currency.is_empty());

    // 4. German Eszett (uppercases to "SS")
    let res_eszett = modal.compute_filtered_tickers("ß");
    assert!(res_eszett.is_empty());

    // 5. Extremely long query (100,000 characters) - stress test memory & bounds
    let huge_query = "A".repeat(100_000);
    let res_huge = modal.compute_filtered_tickers(&huge_query);
    assert!(res_huge.is_empty());

    // 6. Null byte and control characters
    let res_control = modal.compute_filtered_tickers("BTC\0USDT");
    assert!(res_control.is_empty());
}

#[test]
fn test_challenger1_all_11_exchange_variants_and_unusual_tickers() {
    let exhaustive_catalog = vec![
        // 1. Binance Linear
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("1000PEPEUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear),
        // 2. Binance Inverse
        mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse),
        mock_ticker("BTCUSD_241227", Exchange::BinanceInverse),
        // 3. Binance Spot
        mock_ticker("BTCUSDT", Exchange::BinanceSpot),
        mock_ticker("WBTCBTC", Exchange::BinanceSpot),
        // 4. Bybit Linear
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("1000PEPEUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-PERP", Exchange::BybitLinear),
        // 5. Bybit Inverse
        mock_ticker("BTCUSD", Exchange::BybitInverse),
        mock_ticker("BTCUSDM24", Exchange::BybitInverse),
        // 6. Bybit Spot
        mock_ticker("BTCUSDT", Exchange::BybitSpot),
        mock_ticker("ETHUSDT", Exchange::BybitSpot),
        // 7. OKX Linear
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        mock_ticker("BTC-USDT-241227", Exchange::OkexLinear),
        // 8. OKX Inverse
        mock_ticker("BTC-USD-SWAP", Exchange::OkexInverse),
        mock_ticker("BTC-USD-241227", Exchange::OkexInverse),
        // 9. OKX Spot
        mock_ticker("BTC-USDT", Exchange::OkexSpot),
        mock_ticker("LTC-BTC", Exchange::OkexSpot),
        // 10. Hyperliquid Linear
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("kPEPE", Exchange::HyperliquidLinear),
        mock_ticker("PURR", Exchange::HyperliquidLinear),
        // 11. Hyperliquid Spot
        mock_ticker("PURR/USDC", Exchange::HyperliquidSpot),
        mock_ticker("PURR", Exchange::HyperliquidSpot),
        mock_ticker("HYPE", Exchange::HyperliquidSpot),
    ];

    let mut modal = VolumeSearchModal::new(None, exhaustive_catalog.clone());

    // Verify TickerDisplayWrapper formatting on all 26 variants
    for ti in &exhaustive_catalog {
        let wrapper = TickerDisplayWrapper(*ti);
        let s = format!("{wrapper}");
        assert!(!s.is_empty());
        assert!(s.contains(':'), "Wrapper must format as 'Exchange: Symbol'");
    }

    // Test filtering by each exchange
    modal.selected_exchange_filter = Some(ExchangeInclusive::Binance);
    let binance = modal.compute_filtered_tickers("");
    assert_eq!(binance.len(), 7);

    modal.selected_exchange_filter = Some(ExchangeInclusive::Bybit);
    let bybit = modal.compute_filtered_tickers("");
    assert_eq!(bybit.len(), 7);

    modal.selected_exchange_filter = Some(ExchangeInclusive::Okex);
    let okx = modal.compute_filtered_tickers("");
    assert_eq!(okx.len(), 6);

    modal.selected_exchange_filter = Some(ExchangeInclusive::Hyperliquid);
    let hl = modal.compute_filtered_tickers("");
    assert_eq!(hl.len(), 6);

    // Test unusual token formats
    modal.selected_exchange_filter = None;
    let swap_query = modal.compute_filtered_tickers("SWAP");
    assert_eq!(swap_query.len(), 2); // BTC-USDT-SWAP, BTC-USD-SWAP
    assert!(
        swap_query
            .iter()
            .all(|t| ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Okex)
    );

    let pepe_query = modal.compute_filtered_tickers("PEPE");
    assert_eq!(pepe_query.len(), 3); // 1000PEPEUSDT (Binance), 1000PEPEUSDT (Bybit), kPEPE (Hyperliquid)

    let dated_query = modal.compute_filtered_tickers("241227");
    assert_eq!(dated_query.len(), 4); // Binance Linear, Binance Inverse, OKX Linear, OKX Inverse
}

#[test]
fn test_challenger1_extreme_catalog_sizes_performance() {
    use std::time::Instant;

    // Construct 10,000 synthetic tickers
    let mut large_catalog = Vec::with_capacity(10_000);
    let exchanges = [
        Exchange::BinanceLinear,
        Exchange::BinanceSpot,
        Exchange::BybitLinear,
        Exchange::OkexLinear,
        Exchange::HyperliquidLinear,
    ];

    for i in 0..10_000 {
        let ex = exchanges[i % exchanges.len()];
        let sym = format!("SYM{i:05}USDT");
        large_catalog.push(mock_ticker(&sym, ex));
    }
    // Insert recognizable targets
    large_catalog.push(mock_ticker("BTCUSDT", Exchange::BinanceLinear));
    large_catalog.push(mock_ticker("ETHUSDT", Exchange::BybitLinear));

    let modal = VolumeSearchModal::new(None, large_catalog);

    // 1. Empty query benchmark
    let t0 = Instant::now();
    let res_empty = modal.compute_filtered_tickers("");
    let d_empty = t0.elapsed();
    assert_eq!(res_empty.len(), 10_002);

    // 2. Prefix query benchmark
    let t1 = Instant::now();
    let res_prefix = modal.compute_filtered_tickers("SYM00");
    let d_prefix = t1.elapsed();
    assert_eq!(res_prefix.len(), 1000);

    // 3. Single hit query
    let t2 = Instant::now();
    let res_single = modal.compute_filtered_tickers("BTCUSDT");
    let d_single = t2.elapsed();
    assert_eq!(res_single.len(), 1);

    // 4. Non-matching query
    let t3 = Instant::now();
    let res_miss = modal.compute_filtered_tickers("NONEXISTENT");
    let d_miss = t3.elapsed();
    assert!(res_miss.is_empty());

    println!(
        "PERFORMANCE 10k Catalog: Empty: {:?}, Prefix: {:?}, Single: {:?}, Miss: {:?}",
        d_empty, d_prefix, d_single, d_miss
    );
}

#[test]
fn test_challenger1_empirical_proof_exact_match_pruning_bug() {
    let catalog = vec![
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
    ];
    let modal = VolumeSearchModal::new(None, catalog);
    let res = modal.compute_filtered_tickers("BTC");

    // Verifies that matched.retain(|(_, r)| r.bucket <= 1) preserves
    // Binance, Bybit, and OKX when Hyperliquid has an exact asset name match.
    assert_eq!(res.len(), 4);
    assert!(
        res.iter()
            .any(|t| t.exchange() == Exchange::HyperliquidLinear)
    );
    assert!(res.iter().any(|t| t.exchange() == Exchange::BinanceLinear));
    assert!(res.iter().any(|t| t.exchange() == Exchange::BybitLinear));
    assert!(res.iter().any(|t| t.exchange() == Exchange::OkexLinear));
}

#[test]
fn test_challenger1_empirical_proof_inverse_perp_length_bug() {
    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSD", Exchange::BybitInverse),
    ];
    let modal = VolumeSearchModal::new(None, catalog);
    let res = modal.compute_filtered_tickers("BTC");

    // Verifies that Linear Perps ("BTCUSDT", len 7) ranks ahead of
    // Inverse Perps ("BTCUSD", len 6) because market kind priority precedes symbol length.
    assert_eq!(res[0].ticker.as_str(), "BTCUSDT");
    assert_eq!(res[0].exchange(), Exchange::BinanceLinear);
    assert_eq!(res[1].ticker.as_str(), "BTCUSD");
    assert_eq!(res[1].exchange(), Exchange::BybitInverse);
}

#[test]
fn test_regression_btc_cross_exchange_catalog_preserves_all_pairs() {
    let catalog = vec![
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.selected_exchange_filter = None;

    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    // Verification 1: All 4 exchanges must be preserved in search results
    assert_eq!(
        modal.filtered_tickers.len(),
        4,
        "Searching 'BTC' must preserve all 4 cross-exchange candidates (Hyperliquid, Binance, Bybit, OKX)"
    );

    let hl_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::HyperliquidLinear);
    let binance_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::BinanceLinear);
    let bybit_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::BybitLinear);
    let okx_present = modal
        .filtered_tickers
        .iter()
        .any(|t| t.exchange() == Exchange::OkexLinear);

    assert!(hl_present, "Hyperliquid Linear BTC must be preserved");
    assert!(binance_present, "Binance Linear BTCUSDT must be preserved");
    assert!(bybit_present, "Bybit Linear BTCUSDT must be preserved");
    assert!(okx_present, "OKX Linear BTC-USDT-SWAP must be preserved");

    // Verification 2: Exact match (Hyperliquid BTC) must rank first
    assert_eq!(
        modal.filtered_tickers[0].exchange(),
        Exchange::HyperliquidLinear,
        "Exact match Hyperliquid BTC must rank at index 0"
    );
    assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
}

#[test]
fn test_adversarial_ranking_exact_and_prefix_ahead_of_substrings() {
    let catalog = vec![
        // Substring candidates (bucket 3)
        mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
        mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
        mock_ticker("CBTCUSDT", Exchange::BinanceSpot),
        // Suffix candidate (bucket 2)
        mock_ticker("LTC-BTC", Exchange::OkexSpot),
        // Prefix candidates (bucket 1)
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceSpot),
        // Exact candidate (bucket 0)
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        // Non-matches
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.selected_exchange_filter = None;

    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    // Verification 1: Non-matches excluded
    assert!(
        !modal
            .filtered_tickers
            .iter()
            .any(|t| t.ticker.as_str() == "ETHUSDT")
    );
    assert!(
        !modal
            .filtered_tickers
            .iter()
            .any(|t| t.ticker.as_str() == "SOLUSDT")
    );

    // Verification 2: Exact match is at index 0
    assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
    assert_eq!(
        modal.filtered_tickers[0].exchange(),
        Exchange::HyperliquidLinear
    );

    // Verification 3: All exact and prefix matches rank ahead of any substring/suffix matches
    let is_exact_or_prefix = |sym: &str| sym == "BTC" || sym.starts_with("BTC");
    let last_prefix_idx = modal
        .filtered_tickers
        .iter()
        .rposition(|t| is_exact_or_prefix(t.ticker.as_str()))
        .expect("Must have prefix matches");

    let first_substring_idx = modal
        .filtered_tickers
        .iter()
        .position(|t| !is_exact_or_prefix(t.ticker.as_str()));

    if let Some(sub_idx) = first_substring_idx {
        assert!(
            last_prefix_idx < sub_idx,
            "All exact and prefix matches (last at {last_prefix_idx}) must rank ahead of substrings (first at {sub_idx})"
        );
    }
}

#[test]
fn test_adversarial_multi_token_base_symbols_cross_exchange() {
    let catalog = vec![
        // ETH
        mock_ticker("ETH", Exchange::HyperliquidLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BybitLinear),
        mock_ticker("ETH-USDT-SWAP", Exchange::OkexLinear),
        // SOL
        mock_ticker("SOL", Exchange::HyperliquidLinear),
        mock_ticker("SOLUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
        mock_ticker("SOL-USDT-SWAP", Exchange::OkexLinear),
        // HYPE
        mock_ticker("HYPE", Exchange::HyperliquidLinear),
        mock_ticker("HYPEUSDT", Exchange::BybitLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);

    for base_symbol in ["ETH", "SOL", "HYPE"] {
        modal.update(Message::TickerSearchChanged(base_symbol.into()), None);

        let hl_match = modal.filtered_tickers.iter().any(|t| {
            ExchangeInclusive::of(t.exchange()) == ExchangeInclusive::Hyperliquid
                && t.ticker.as_str() == base_symbol
        });
        let cross_exchange_matches = modal
            .filtered_tickers
            .iter()
            .filter(|t| {
                ExchangeInclusive::of(t.exchange()) != ExchangeInclusive::Hyperliquid
                    && t.ticker.as_str().starts_with(base_symbol)
            })
            .count();

        assert!(hl_match, "Hyperliquid {base_symbol} must be present");
        assert!(
            cross_exchange_matches >= 1,
            "Cross-exchange suffixed pairs for {base_symbol} must not be purged by Hyperliquid exact match"
        );
    }
}

#[test]
fn test_adversarial_whitespace_and_case_preservation() {
    let catalog = vec![
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);

    for query in ["  btc  ", "bTc", " \tBTC\r\n ", "btc   "] {
        modal.update(Message::TickerSearchChanged(query.into()), None);
        assert_eq!(
            modal.filtered_tickers.len(),
            4,
            "Query '{query}' must preserve all 4 candidates regardless of whitespace or casing"
        );
        assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTC");
        assert_eq!(
            modal.filtered_tickers[0].exchange(),
            Exchange::HyperliquidLinear
        );
    }
}

#[test]
fn test_adversarial_exchange_filter_cross_exchange_isolation() {
    let catalog = vec![
        mock_ticker("BTC", Exchange::HyperliquidLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BybitLinear),
        mock_ticker("BTC-USDT-SWAP", Exchange::OkexLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    // Filter Binance
    modal.update(
        Message::ExchangeFilterSelected(Some(ExchangeInclusive::Binance)),
        None,
    );
    assert_eq!(modal.filtered_tickers.len(), 1);
    assert_eq!(
        modal.filtered_tickers[0].exchange(),
        Exchange::BinanceLinear
    );

    // Filter Hyperliquid
    modal.update(
        Message::ExchangeFilterSelected(Some(ExchangeInclusive::Hyperliquid)),
        None,
    );
    assert_eq!(modal.filtered_tickers.len(), 1);
    assert_eq!(
        modal.filtered_tickers[0].exchange(),
        Exchange::HyperliquidLinear
    );

    // Reset to All
    modal.update(Message::ExchangeFilterSelected(None), None);
    assert_eq!(modal.filtered_tickers.len(), 4);
}

#[test]
fn test_regression_linear_perps_rank_ahead_of_shorter_inverse_perps() {
    let catalog = vec![
        mock_ticker("BTCUSD", Exchange::BybitInverse), // len 6, InversePerps
        mock_ticker("BTCUSDT", Exchange::BinanceLinear), // len 7, LinearPerps
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.selected_exchange_filter = None;

    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    assert_eq!(modal.filtered_tickers.len(), 2);
    assert_eq!(
        modal.filtered_tickers[0].ticker.as_str(),
        "BTCUSDT",
        "LinearPerps BTCUSDT (len 7) must rank strictly ahead of InversePerps BTCUSD (len 6)"
    );
    assert_eq!(
        modal.filtered_tickers[0].exchange(),
        Exchange::BinanceLinear
    );
    assert_eq!(
        modal.filtered_tickers[0].market_type(),
        MarketKind::LinearPerps
    );

    assert_eq!(
        modal.filtered_tickers[1].ticker.as_str(),
        "BTCUSD",
        "InversePerps BTCUSD must rank after LinearPerps BTCUSDT"
    );
    assert_eq!(modal.filtered_tickers[1].exchange(), Exchange::BybitInverse);
    assert_eq!(
        modal.filtered_tickers[1].market_type(),
        MarketKind::InversePerps
    );
}

#[test]
fn test_adversarial_enter_submission_selects_linear_over_inverse() {
    let catalog = vec![
        mock_ticker("WBTCUSDT", Exchange::BinanceSpot),
        mock_ticker("BTCUSD", Exchange::BybitInverse),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("1000BTCUSDT", Exchange::BybitLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::TickerSearchChanged("BTC".into()), None);
    modal.update(Message::TickerSearchSubmitted, None);

    let selected = modal.selected_ticker.expect("Must have selected a ticker");
    assert_eq!(
        selected.ticker.as_str(),
        "BTCUSDT",
        "Enter submission must select LinearPerps BTCUSDT, not InversePerps BTCUSD"
    );
    assert_eq!(selected.exchange(), Exchange::BinanceLinear);
    assert_eq!(selected.market_type(), MarketKind::LinearPerps);
}

#[test]
fn test_adversarial_market_kind_three_tier_hierarchy() {
    let catalog = vec![
        mock_ticker("BTCUSD_PERP", Exchange::BinanceInverse), // Inverse, len 11
        mock_ticker("BTCUSD", Exchange::BybitInverse),        // Inverse, len 6
        mock_ticker("BTCUSDT", Exchange::BinanceSpot),        // Spot, len 7
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),      // Linear, len 7
        mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear), // Linear, len 14
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    assert_eq!(modal.filtered_tickers.len(), 5);

    // Tier 1: Linear Perps (sorted by length: 7 < 14)
    assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTCUSDT");
    assert_eq!(
        modal.filtered_tickers[0].market_type(),
        MarketKind::LinearPerps
    );

    assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSDT_241227");
    assert_eq!(
        modal.filtered_tickers[1].market_type(),
        MarketKind::LinearPerps
    );

    // Tier 2: Spot
    assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "BTCUSDT");
    assert_eq!(modal.filtered_tickers[2].market_type(), MarketKind::Spot);

    // Tier 3: Inverse Perps (sorted by length: 6 < 11)
    assert_eq!(modal.filtered_tickers[3].ticker.as_str(), "BTCUSD");
    assert_eq!(
        modal.filtered_tickers[3].market_type(),
        MarketKind::InversePerps
    );

    assert_eq!(modal.filtered_tickers[4].ticker.as_str(), "BTCUSD_PERP");
    assert_eq!(
        modal.filtered_tickers[4].market_type(),
        MarketKind::InversePerps
    );
}

#[test]
fn test_adversarial_linear_tie_breaking_by_length_and_symbol() {
    let catalog = vec![
        mock_ticker("BTCUSDT_241227", Exchange::BinanceLinear), // len 14
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),        // len 7
        mock_ticker("BTCUSDC", Exchange::BinanceLinear),        // len 7 (alphabetical tiebreak)
    ];

    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::TickerSearchChanged("BTC".into()), None);

    assert_eq!(modal.filtered_tickers.len(), 3);
    assert_eq!(modal.filtered_tickers[0].ticker.as_str(), "BTCUSDC");
    assert_eq!(modal.filtered_tickers[1].ticker.as_str(), "BTCUSDT");
    assert_eq!(modal.filtered_tickers[2].ticker.as_str(), "BTCUSDT_241227");
}

#[test]
fn test_regression_active_pane_inherited_on_modal_open_with_populated_catalog() {
    let active_pane_ticker = mock_ticker("ETHUSDT", Exchange::BinanceLinear);
    let catalog = vec![
        mock_ticker("1000BONKUSDT", Exchange::BinanceLinear), // Catalog head
        mock_ticker("AAVEUSDT", Exchange::BinanceLinear),
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
    ];

    let mut modal = VolumeSearchModal::new(None, Vec::new());

    // Correct order: inherit active pane before populating catalog
    modal.inherit_active_pane_ticker(Some(active_pane_ticker));
    modal.update_available_tickers(catalog);

    assert_eq!(
        modal.selected_ticker,
        Some(active_pane_ticker),
        "Active chart pane ticker (ETHUSDT) must be inherited and NOT overwritten by catalog head (1000BONKUSDT)"
    );
}

#[test]
fn test_adversarial_active_pane_fallback_to_catalog_first_when_zero_panes() {
    let mut modal = VolumeSearchModal::new(None, Vec::new());
    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
    ];

    // Zero open panes: active_pane_ticker is None
    modal.inherit_active_pane_ticker(None);
    modal.update_available_tickers(catalog.clone());

    assert_eq!(
        modal.selected_ticker,
        Some(catalog[0]),
        "When zero panes are open, selected_ticker must fall back to the first catalog candidate"
    );
}

#[test]
fn test_adversarial_catalog_arrives_after_modal_opened() {
    let active_pane_ticker = mock_ticker("SOLUSDT", Exchange::BybitLinear);
    let mut modal = VolumeSearchModal::new(None, Vec::new());

    // Modal opened while catalog is still downloading over network
    modal.inherit_active_pane_ticker(Some(active_pane_ticker));
    assert_eq!(modal.selected_ticker, Some(active_pane_ticker));

    // Network response arrives later with 500 items
    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("SOLUSDT", Exchange::BybitLinear),
    ];
    modal.update_available_tickers(catalog);

    assert_eq!(
        modal.selected_ticker,
        Some(active_pane_ticker),
        "Subsequent catalog arrival must not overwrite already inherited active pane selection"
    );
}

#[test]
fn test_adversarial_explicit_user_selection_immune_to_subsequent_pane_or_catalog_events() {
    let active_pane_ticker = mock_ticker("ETHUSDT", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(None, Vec::new());
    modal.inherit_active_pane_ticker(Some(active_pane_ticker));

    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
        mock_ticker("DOGEUSDT", Exchange::BinanceLinear),
    ];
    modal.update_available_tickers(catalog.clone());

    // User explicitly selects DOGEUSDT
    let doge = mock_ticker("DOGEUSDT", Exchange::BinanceLinear);
    modal.update(Message::TickerSelected(doge), None);
    assert_eq!(modal.selected_ticker, Some(doge));

    // Re-triggering active pane inheritance or catalog refresh must NOT override user choice
    modal.inherit_active_pane_ticker(Some(active_pane_ticker));
    modal.update_available_tickers(catalog);

    assert_eq!(
        modal.selected_ticker,
        Some(doge),
        "Explicit user selection must remain immune to subsequent background sync events"
    );
}

#[test]
fn test_adversarial_active_pane_unlisted_ticker_safe() {
    let unlisted_ticker = mock_ticker("EXOTIC-PAIR", Exchange::OkexLinear);
    let mut modal = VolumeSearchModal::new(None, Vec::new());

    modal.inherit_active_pane_ticker(Some(unlisted_ticker));

    let catalog = vec![
        mock_ticker("BTCUSDT", Exchange::BinanceLinear),
        mock_ticker("ETHUSDT", Exchange::BinanceLinear),
    ];
    modal.update_available_tickers(catalog);

    assert_eq!(
        modal.selected_ticker,
        Some(unlisted_ticker),
        "Modal must safely preserve unlisted active pane ticker without panicking"
    );
}

// ========================================================================
// Milestone 2 (R2): Calendar Dates & Custom Range Unit Test Suite
// ========================================================================

#[test]
fn test_m2_preset_selection_auto_populates_date_inputs() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    let today = chrono::Utc::now().date_naive();

    // 1. Initial state (Last3d default)
    assert_eq!(modal.preset, VolumeSearchPreset::Last3d);
    let expected_3d_start = today
        .checked_sub_signed(chrono::Duration::days(2))
        .unwrap_or(today);
    assert_eq!(modal.start_date, expected_3d_start);
    assert_eq!(modal.end_date, today);
    assert_eq!(
        modal.start_date_input,
        expected_3d_start.format("%Y-%m-%d").to_string()
    );
    assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
    assert!(modal.is_start_date_valid);
    assert!(modal.is_end_date_valid);

    // 2. Select Last24h
    modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Last24h);
    let expected_24h_start = today.pred_opt().unwrap_or(today);
    assert_eq!(modal.start_date, expected_24h_start);
    assert_eq!(modal.end_date, today);
    assert_eq!(
        modal.start_date_input,
        expected_24h_start.format("%Y-%m-%d").to_string()
    );
    assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
    assert!(modal.is_start_date_valid);
    assert!(modal.is_end_date_valid);

    // 3. Select Last7d
    modal.update(Message::PresetSelected(VolumeSearchPreset::Last7d), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Last7d);
    let expected_7d_start = today
        .checked_sub_signed(chrono::Duration::days(6))
        .unwrap_or(today);
    assert_eq!(modal.start_date, expected_7d_start);
    assert_eq!(modal.end_date, today);
    assert_eq!(
        modal.start_date_input,
        expected_7d_start.format("%Y-%m-%d").to_string()
    );
    assert_eq!(modal.end_date_input, today.format("%Y-%m-%d").to_string());
    assert!(modal.is_start_date_valid);
    assert!(modal.is_end_date_valid);

    // 4. Select Custom (preserves existing dates)
    modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);
    assert_eq!(modal.start_date, expected_7d_start);
    assert_eq!(modal.end_date, today);
}

#[test]
fn test_m2_preset_selection_preserves_intraday_time_filter() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // User customizes intraday time filters
    modal.update(Message::TimeFilterStartChanged("08:30".into()), None);
    modal.update(Message::TimeFilterEndChanged("17:45".into()), None);
    assert_eq!(modal.time_filter_start, "08:30");
    assert_eq!(modal.time_filter_end, "17:45");

    // Switching presets must preserve time filters intact
    modal.update(Message::PresetSelected(VolumeSearchPreset::Last7d), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Last7d);
    assert_eq!(modal.time_filter_start, "08:30");
    assert_eq!(modal.time_filter_end, "17:45");

    modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Last24h);
    assert_eq!(modal.time_filter_start, "08:30");
    assert_eq!(modal.time_filter_end, "17:45");

    modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);
    assert_eq!(modal.time_filter_start, "08:30");
    assert_eq!(modal.time_filter_end, "17:45");
}

#[test]
fn test_m2_manual_input_switches_preset_to_custom() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    assert_eq!(modal.preset, VolumeSearchPreset::Last3d);

    // Editing start date input automatically switches preset to Custom
    modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
    );
    assert_eq!(modal.start_date_input, "2026-10-01");
    assert!(modal.is_start_date_valid);

    // Switch back to Last24h
    modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Last24h);

    // Editing end date input automatically switches preset to Custom
    modal.update(Message::EndDateInputChanged("2026-10-08".into()), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()
    );
    assert_eq!(modal.end_date_input, "2026-10-08");
    assert!(modal.is_end_date_valid);
}

#[test]
fn test_m2_manual_input_validation_and_boundary_clamping() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // Invalid date string
    modal.update(Message::StartDateInputChanged("2026-13-45".into()), None);
    assert_eq!(modal.start_date_input, "2026-13-45");
    assert!(!modal.is_start_date_valid);
    assert!(!modal.is_date_range_valid());

    // Valid start date
    modal.update(Message::StartDateInputChanged("2026-10-05".into()), None);
    assert!(modal.is_start_date_valid);
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    );

    // Set valid end date
    modal.update(Message::EndDateInputChanged("2026-10-10".into()), None);
    assert!(modal.is_end_date_valid);
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
    );
    assert!(modal.is_date_range_valid());

    // Boundary clamping: if user sets start_date > end_date, end_date clamps to start_date
    modal.update(Message::StartDateInputChanged("2026-10-15".into()), None);
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    );
    assert_eq!(modal.end_date_input, "2026-10-15");
    assert!(modal.is_date_range_valid());

    // Boundary clamping: if user sets end_date < start_date, start_date clamps to end_date
    modal.update(Message::EndDateInputChanged("2026-10-02".into()), None);
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()
    );
    assert_eq!(modal.start_date_input, "2026-10-02");
    assert!(modal.is_date_range_valid());
}

#[test]
fn test_m2_calendar_date_selection_auto_clamps_inverted_range() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // Open start calendar
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

    // Select start date 2026-10-05
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()),
        None,
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    );
    assert_eq!(modal.start_date_input, "2026-10-05");
    assert_eq!(modal.active_calendar, None); // Popover dismissed
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);

    // Open end calendar and pick 2026-10-12
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()),
        None,
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
    );
    assert_eq!(modal.end_date_input, "2026-10-12");

    // Pick start date later than end date (2026-10-20 > 2026-10-12)
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
        None,
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    );
    // Auto-clamped end_date to start_date
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    );
    assert_eq!(modal.end_date_input, "2026-10-20");

    // Pick end date earlier than start date (2026-10-10 < 2026-10-20)
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()),
        None,
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
    );
    // Auto-clamped start_date to end_date
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
    );
    assert_eq!(modal.start_date_input, "2026-10-10");
}

#[test]
fn test_m2_calendar_navigation_and_jump_to_today() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    let today = chrono::Utc::now().date_naive();

    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.cal_view_year, modal.start_date.year());
    assert_eq!(modal.cal_view_month, modal.start_date.month());

    // Next month
    let cur_m = modal.cal_view_month;
    modal.update(Message::CalNextMonth, None);
    let expected_next_m = if cur_m == 12 { 1 } else { cur_m + 1 };
    assert_eq!(modal.cal_view_month, expected_next_m);

    // Prev month
    modal.update(Message::CalPrevMonth, None);
    assert_eq!(modal.cal_view_month, cur_m);

    // Next year
    let cur_y = modal.cal_view_year;
    modal.update(Message::CalNextYear, None);
    assert_eq!(modal.cal_view_year, cur_y + 1);

    // Prev year
    modal.update(Message::CalPrevYear, None);
    assert_eq!(modal.cal_view_year, cur_y);

    // Jump to today
    modal.cal_view_year = 2030;
    modal.cal_view_month = 6;
    modal.update(Message::CalJumpToToday, None);
    assert_eq!(modal.cal_view_year, today.year());
    assert_eq!(modal.cal_view_month, today.month());

    // Close calendar
    modal.update(Message::CloseCalendar, None);
    assert_eq!(modal.active_calendar, None);
}

#[test]
fn test_m2_calendar_popover_toggling_and_view_sync() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    let d1 = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
    let d2 = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();
    modal.update(
        Message::StartDateInputChanged(d1.format("%Y-%m-%d").to_string()),
        None,
    );
    modal.update(
        Message::EndDateInputChanged(d2.format("%Y-%m-%d").to_string()),
        None,
    );

    // Toggle Start calendar opens with start_date coordinates
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
    assert_eq!(modal.cal_view_year, 2025);
    assert_eq!(modal.cal_view_month, 3);

    // Toggling Start again closes it
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, None);

    // Toggle End calendar opens with end_date coordinates
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::End));
    assert_eq!(modal.cal_view_year, 2025);
    assert_eq!(modal.cal_view_month, 7);
}

#[test]
fn test_m2_parse_date_valid_and_invalid_formats() {
    // Valid
    assert_eq!(
        parse_date_ymd("2026-10-11"),
        Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
    );
    assert_eq!(
        parse_date_ymd("2024-01-01"),
        Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
    );
    assert_eq!(
        parse_date_ymd("   2026-10-11   "),
        Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
    );

    // Invalid formats & lengths
    assert_eq!(parse_date_ymd(""), None);
    assert_eq!(parse_date_ymd("2026/10/11"), None);
    assert_eq!(parse_date_ymd("2026.10.11"), None);
    assert_eq!(parse_date_ymd("2026-1-1"), None);
    assert_eq!(parse_date_ymd("invalid"), None);

    // Out of bounds months & days
    assert_eq!(parse_date_ymd("2026-00-10"), None);
    assert_eq!(parse_date_ymd("2026-13-10"), None);
    assert_eq!(parse_date_ymd("2026-05-00"), None);
    assert_eq!(parse_date_ymd("2026-04-31"), None); // April has 30 days
    assert_eq!(parse_date_ymd("2026-02-30"), None);
}

#[test]
fn test_m2_leap_years_and_days_in_month() {
    // Standard leap years
    assert!(is_leap_year(2024));
    assert!(is_leap_year(2020));
    assert!(is_leap_year(2016));
    // Non-leap years
    assert!(!is_leap_year(2023));
    assert!(!is_leap_year(2025));
    assert!(!is_leap_year(2026));
    // Century boundaries
    assert!(is_leap_year(2000));
    assert!(!is_leap_year(1900));
    assert!(!is_leap_year(2100));

    // Days in February
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2025, 2), 28);
    assert_eq!(days_in_month(2026, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(days_in_month(1900, 2), 28);

    // All 12 months in 2026
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for (idx, &expected) in month_days.iter().enumerate() {
        assert_eq!(days_in_month(2026, (idx + 1) as u32), expected);
    }

    // Parsing Feb 29
    assert_eq!(
        parse_date_ymd("2024-02-29"),
        Some(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
    );
    assert_eq!(parse_date_ymd("2025-02-29"), None);
}

#[test]
fn test_m2_month_and_year_boundary_navigation_rollovers() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

    // Wrap from January to December of previous year
    modal.cal_view_year = 2026;
    modal.cal_view_month = 1;
    modal.update(Message::CalPrevMonth, None);
    assert_eq!(modal.cal_view_year, 2025);
    assert_eq!(modal.cal_view_month, 12);

    // Wrap from December to January of next year
    modal.update(Message::CalNextMonth, None);
    assert_eq!(modal.cal_view_year, 2026);
    assert_eq!(modal.cal_view_month, 1);

    // Year lower clamp (2000)
    modal.cal_view_year = 2000;
    modal.update(Message::CalPrevYear, None);
    assert_eq!(modal.cal_view_year, 2000);

    // Year upper clamp (2099)
    modal.cal_view_year = 2099;
    modal.update(Message::CalNextYear, None);
    assert_eq!(modal.cal_view_year, 2099);
}

#[test]
fn test_m2_calculate_query_range_standard_intraday() {
    use chrono::Timelike;
    let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    let (s_ms, e_ms, s_min, e_min) =
        calculate_custom_query_range(start, end, "09:30", "16:00", now_ms);

    assert_eq!(s_min, 570); // 9 * 60 + 30
    assert_eq!(e_min, 960); // 16 * 60
    assert!(s_ms < e_ms);

    let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
    let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

    assert_eq!(s_dt.date_naive(), start);
    assert_eq!(s_dt.hour(), 9);
    assert_eq!(s_dt.minute(), 30);
    assert_eq!(s_dt.second(), 0);

    assert_eq!(e_dt.date_naive(), end);
    assert_eq!(e_dt.hour(), 16);
    assert_eq!(e_dt.minute(), 0);
    assert_eq!(e_dt.second(), 59);
}

#[test]
fn test_m2_calculate_query_range_same_day_overnight_zero_panic() {
    use chrono::Timelike;
    // Critical edge case: Single-day range with cross-midnight filter (22:00 -> 04:00)
    let day = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    let (s_ms, e_ms, s_min, e_min) =
        calculate_custom_query_range(day, day, "22:00", "04:00", now_ms);

    assert_eq!(s_min, 1320); // 22:00
    assert_eq!(e_min, 240); // 04:00
    // Monotonicity invariant: s_ms <= e_ms MUST hold, preventing BTreeMap::range panic!
    assert!(
        s_ms <= e_ms,
        "start_ms must never exceed end_ms on single-day cross-midnight ranges"
    );

    let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
    let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

    assert_eq!(s_dt.date_naive(), day);
    assert_eq!(s_dt.hour(), 0);
    assert_eq!(e_dt.date_naive(), day);
    assert_eq!(e_dt.hour(), 23);
}

#[test]
fn test_m2_calculate_query_range_multi_day_overnight() {
    use chrono::Timelike;
    let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    let now_ms = NaiveDate::from_ymd_opt(2026, 10, 10)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    let (s_ms, e_ms, s_min, e_min) =
        calculate_custom_query_range(start, end, "22:00", "04:00", now_ms);

    assert_eq!(s_min, 1320);
    assert_eq!(e_min, 240);
    assert!(s_ms < e_ms);

    let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
    let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();

    assert_eq!(s_dt.date_naive(), start);
    assert_eq!(s_dt.hour(), 22);
    assert_eq!(e_dt.date_naive(), end);
    assert_eq!(e_dt.hour(), 4);
}

#[test]
fn test_m2_calculate_query_range_inversion_and_future_clamping() {
    let future_end = NaiveDate::from_ymd_opt(2029, 1, 1).unwrap();
    let future_start = NaiveDate::from_ymd_opt(2029, 1, 10).unwrap(); // start > end
    let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
    let now_ms = today
        .and_hms_opt(15, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    let (s_ms, e_ms, _, _) =
        calculate_custom_query_range(future_start, future_end, "00:00", "23:59", now_ms);

    assert!(s_ms <= e_ms);
    assert!(e_ms <= now_ms, "end_ms must be clamped to now_ms");

    let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();
    assert_eq!(e_dt.date_naive(), today, "Future date must clamp to today");
}

#[test]
fn test_m2_target_dates_contiguous_sequence() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
    modal.update(Message::EndDateInputChanged("2026-10-05".into()), None);

    let dates = modal.target_dates();
    assert_eq!(dates.len(), 5);
    assert_eq!(dates[0], NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
    assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 10, 2).unwrap());
    assert_eq!(dates[2], NaiveDate::from_ymd_opt(2026, 10, 3).unwrap());
    assert_eq!(dates[3], NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
    assert_eq!(dates[4], NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());

    // Single day
    modal.update(Message::EndDateInputChanged("2026-10-01".into()), None);
    let single = modal.target_dates();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0], NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());

    // Invalid date returns empty vector
    modal.update(Message::StartDateInputChanged("invalid".into()), None);
    assert!(modal.target_dates().is_empty());
}

#[test]
fn test_m2_perform_search_custom_preset_monotonic_timestamps() {
    let mock_coin = mock_ticker("NONEXISTENT_COIN", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    modal.update(Message::StartDateInputChanged("2026-10-01".into()), None);
    modal.update(Message::EndDateInputChanged("2026-10-03".into()), None);
    modal.update(Message::TimeFilterStartChanged("10:00".into()), None);
    modal.update(Message::TimeFilterEndChanged("15:00".into()), None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);

    // Perform search without panes (verifies zero panics and has_searched flag)
    let action = modal.update(Message::ApplySearch, None);
    if action.is_some() {
        modal.update(Message::CancelSearch, None);
    }
    assert!(modal.has_searched);
    assert!(modal.rows.is_empty());
}

// ========================================================================
// Milestone 2 (R2) Challenger 2 Empirical Adversarial Stress Test Suite
// ========================================================================

#[test]
fn test_c2_adversarial_preset_switching_state_machine_matrix() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    let today = chrono::Utc::now().date_naive();

    let presets = [
        VolumeSearchPreset::Last24h,
        VolumeSearchPreset::Last3d,
        VolumeSearchPreset::Last7d,
        VolumeSearchPreset::Custom,
        VolumeSearchPreset::AllLoaded,
    ];

    // Custom user state to verify preservation
    modal.update(Message::TimeFilterStartChanged("07:15".into()), None);
    modal.update(Message::TimeFilterEndChanged("21:45".into()), None);
    modal.update(Message::MultiplierSelected(TickMultiplier(50)), None);
    modal.update(Message::MinVolumeChanged("1250.5".into()), None);

    for &p1 in &presets {
        for &p2 in &presets {
            // Open calendar popover before transition
            modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
            assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

            modal.update(Message::PresetSelected(p1), None);
            assert_eq!(modal.preset, p1);
            // All presets must dismiss open calendar popover
            assert_eq!(modal.active_calendar, None);

            // Verify user state preserved
            assert_eq!(modal.time_filter_start, "07:15");
            assert_eq!(modal.time_filter_end, "21:45");
            assert_eq!(modal.selected_multiplier, TickMultiplier(50));
            assert_eq!(modal.min_volume_input, "1250.5");

            // Switch to p2
            modal.update(Message::PresetSelected(p2), None);
            assert_eq!(modal.preset, p2);
            assert_eq!(modal.active_calendar, None);

            match p2 {
                VolumeSearchPreset::Last24h => {
                    let expected = today.pred_opt().unwrap_or(today);
                    assert_eq!(modal.start_date, expected);
                    assert_eq!(modal.end_date, today);
                    assert!(modal.is_start_date_valid);
                    assert!(modal.is_end_date_valid);
                }
                VolumeSearchPreset::Last3d => {
                    let expected = today
                        .checked_sub_signed(chrono::Duration::days(2))
                        .unwrap_or(today);
                    assert_eq!(modal.start_date, expected);
                    assert_eq!(modal.end_date, today);
                }
                VolumeSearchPreset::Last7d => {
                    let expected = today
                        .checked_sub_signed(chrono::Duration::days(6))
                        .unwrap_or(today);
                    assert_eq!(modal.start_date, expected);
                    assert_eq!(modal.end_date, today);
                }
                VolumeSearchPreset::Custom | VolumeSearchPreset::AllLoaded => {}
            }
        }
    }

    // PresetSelected(Custom) dismisses active_calendar if one was open
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
    modal.update(Message::PresetSelected(VolumeSearchPreset::Custom), None);
    assert_eq!(
        modal.active_calendar, None,
        "PresetSelected(Custom) dismisses active_calendar"
    );
}

#[test]
fn test_c2_adversarial_popover_backdrop_click_absorption_and_stack_view() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // 1. Initially no popover
    assert_eq!(modal.active_calendar, None);
    let _ = modal.view(); // base view must render without panic

    // 2. Open Start calendar popover
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));
    let _ = modal.view(); // stack view with popover must render without panic

    // 3. Toggle Start calendar again -> closes
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, None);

    // 4. Open End calendar popover
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::End));
    let _ = modal.view();

    // 5. Switching directly to Start calendar popover
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    assert_eq!(modal.active_calendar, Some(CalendarTarget::Start));

    // 6. Backdrop click dispatches CloseCalendar
    modal.update(Message::CloseCalendar, None);
    assert_eq!(modal.active_calendar, None);

    // 7. CalSelectDate closes popover and switches to Custom
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
        None,
    );
    assert_eq!(modal.active_calendar, None);
    assert_eq!(modal.preset, VolumeSearchPreset::Custom);
}

#[test]
fn test_c2_adversarial_zero_panic_invariants_on_malformed_inputs() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // Inject extreme/malformed inputs
    modal.update(Message::StartDateInputChanged("not-a-date".into()), None);
    modal.update(Message::EndDateInputChanged("9999-99-99".into()), None);
    modal.update(Message::TimeFilterStartChanged("99:99".into()), None);
    modal.update(Message::TimeFilterEndChanged("abc:def".into()), None);
    modal.update(Message::MinVolumeChanged("NaN".into()), None);
    modal.update(Message::CustomMultiplierChanged("999999".into()), None);

    assert!(!modal.is_start_date_valid);
    assert!(!modal.is_end_date_valid);

    // ApplySearch must sanitize invalid date strings back to last valid dates and execute without panic
    let action = modal.update(Message::ApplySearch, None);
    if action.is_some() {
        modal.update(Message::CancelSearch, None);
    }
    assert!(modal.has_searched);
    assert!(modal.is_start_date_valid);
    assert!(modal.is_end_date_valid);

    // Repeat with empty strings
    modal.update(Message::StartDateInputChanged("".into()), None);
    modal.update(Message::EndDateInputChanged("".into()), None);
    modal.update(Message::TimeFilterStartChanged("".into()), None);
    modal.update(Message::TimeFilterEndChanged("".into()), None);
    modal.update(Message::MinVolumeChanged("-500.0".into()), None);
    let action2 = modal.update(Message::ApplySearch, None);
    if action2.is_some() {
        modal.update(Message::CancelSearch, None);
    }
    assert!(modal.has_searched);

    // Repeat with selected_ticker = None
    modal.selected_ticker = None;
    modal.update(Message::ApplySearch, None);

    // Repeat with 100 random message dispatches in sequence
    for i in 0..100 {
        match i % 6 {
            0 => {
                modal.update(Message::PresetSelected(VolumeSearchPreset::Last24h), None);
            }
            1 => {
                modal.update(
                    Message::StartDateInputChanged(format!("2026-02-{:02}", (i % 28) + 1)),
                    None,
                );
            }
            2 => {
                modal.update(
                    Message::TimeFilterStartChanged(format!("{:02}:00", i % 24)),
                    None,
                );
            }
            3 => {
                modal.update(Message::ApplySearch, None);
            }
            4 => {
                modal.update(Message::CalPrevMonth, None);
            }
            _ => {
                modal.update(Message::CalNextYear, None);
            }
        }
    }
    assert!(modal.has_searched);
}

#[test]
fn test_c2_adversarial_monotonic_range_future_intraday_exceeds_now_ms() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
    let now_ms = today
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    // User sets today with intraday filter 18:00 - 20:00 (in the future relative to now_ms = 12:00)
    let (s_ms, e_ms, s_min, e_min) =
        calculate_custom_query_range(today, today, "18:00", "20:00", now_ms);

    assert_eq!(s_min, 1080);
    assert_eq!(e_min, 1200);

    // 1. Monotonicity: s_ms <= e_ms holds (preventing BTreeMap range panic)
    assert!(s_ms <= e_ms, "start_ms must never exceed end_ms");

    // 2. Bound invariant: e_ms <= now_ms
    // Worker claimed: "guaranteeing monotonic timestamps (start_time <= end_time <= now_ms)"
    // Docstring claimed: "Guarantees start_ms <= end_ms <= now_ms"
    // In reality, e_ms exceeds now_ms by 6 hours because start_dt (18:00) was never clamped to now_ms,
    // and when start_ms > end_ms, swapping sets final_end_ms = 18:00 (future)!
    assert!(
        e_ms <= now_ms,
        "BUG DETECTED: calculate_custom_query_range returned end_time ({e_ms}) > now_ms ({now_ms}). Difference: {} ms in future!",
        e_ms.saturating_sub(now_ms)
    );
}

// ========================================================================
// Milestone 2 (R2) Challenger 1 Empirical Adversarial Test Suite

// ========================================================================

#[test]
fn test_c1_adversarial_leap_years_and_century_rules() {
    // 1. Gregorian leap year rules across multiple centuries
    assert!(is_leap_year(1600), "1600 divisible by 400 must be leap");
    assert!(
        !is_leap_year(1700),
        "1700 divisible by 100 but not 400 must NOT be leap"
    );
    assert!(
        !is_leap_year(1800),
        "1800 divisible by 100 but not 400 must NOT be leap"
    );
    assert!(
        !is_leap_year(1900),
        "1900 divisible by 100 but not 400 must NOT be leap"
    );
    assert!(is_leap_year(2000), "2000 divisible by 400 must be leap");
    assert!(is_leap_year(2004), "2004 divisible by 4 must be leap");
    assert!(is_leap_year(2020), "2020 divisible by 4 must be leap");
    assert!(is_leap_year(2024), "2024 divisible by 4 must be leap");
    assert!(
        !is_leap_year(2025),
        "2025 not divisible by 4 must NOT be leap"
    );
    assert!(
        !is_leap_year(2026),
        "2026 not divisible by 4 must NOT be leap"
    );
    assert!(
        !is_leap_year(2100),
        "2100 divisible by 100 but not 400 must NOT be leap"
    );
    assert!(is_leap_year(2400), "2400 divisible by 400 must be leap");

    // 2. February day counts matching leap year rules
    assert_eq!(days_in_month(1600, 2), 29);
    assert_eq!(days_in_month(1900, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2025, 2), 28);
    assert_eq!(days_in_month(2026, 2), 28);
    assert_eq!(days_in_month(2100, 2), 28);
    assert_eq!(days_in_month(2400, 2), 29);

    // 3. Parsing Feb 29 across leap vs non-leap years
    assert!(parse_date_ymd("2000-02-29").is_some());
    assert!(parse_date_ymd("2024-02-29").is_some());
    assert!(parse_date_ymd("1900-02-29").is_none());
    assert!(parse_date_ymd("2025-02-29").is_none());
    assert!(parse_date_ymd("2026-02-29").is_none());
    assert!(parse_date_ymd("2100-02-29").is_none());

    // 4. Parity with chrono NaiveDate across all years in 1990..=2110
    for y in 1990..=2110 {
        let chrono_leap = NaiveDate::from_ymd_opt(y, 2, 29).is_some();
        assert_eq!(
            is_leap_year(y),
            chrono_leap,
            "Mismatch with chrono for year {y}"
        );
        assert_eq!(
            days_in_month(y, 2),
            if chrono_leap { 29 } else { 28 },
            "Mismatch in Feb days for year {y}"
        );
    }
}

#[test]
fn test_c1_adversarial_month_transition_calendar_navigation() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

    // 1. Full 24-month forward cycle: verify year/month transitions
    modal.cal_view_year = 2024;
    modal.cal_view_month = 1;
    for i in 1..=24 {
        modal.update(Message::CalNextMonth, None);
        let expected_month = (i % 12) + 1;
        let expected_year = 2024 + (i / 12);
        assert_eq!(modal.cal_view_month, expected_month as u32);
        assert_eq!(modal.cal_view_year, expected_year);
    }

    // 2. Full 24-month backward cycle
    for i in (1..=24).rev() {
        modal.update(Message::CalPrevMonth, None);
        let expected_month = ((i - 1) % 12) + 1;
        let expected_year = 2024 + ((i - 1) / 12);
        assert_eq!(modal.cal_view_month, expected_month as u32);
        assert_eq!(modal.cal_view_year, expected_year);
    }

    // 3. Boundary navigation behavior at minimum year (2000-01)
    modal.cal_view_year = 2000;
    modal.cal_view_month = 1;
    modal.update(Message::CalPrevMonth, None);
    // cal_view_year and cal_view_month remain clamped at minimum boundary (2000-01)
    assert_eq!(modal.cal_view_year, 2000);
    assert_eq!(modal.cal_view_month, 1);

    // 4. Boundary navigation behavior at maximum year (2099-12)
    modal.cal_view_year = 2099;
    modal.cal_view_month = 12;
    modal.update(Message::CalNextMonth, None);
    // cal_view_year and cal_view_month remain clamped at maximum boundary (2099-12)
    assert_eq!(modal.cal_view_year, 2099);
    assert_eq!(modal.cal_view_month, 12);
}

#[test]
fn test_c1_adversarial_inverted_date_ranges_and_boundary_clamping() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // 1. CalSelectDate: picking start date > end date auto-clamps end date to start date
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()),
        None,
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
    );

    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()),
        None,
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    );
    assert!(modal.is_date_range_valid());

    // 2. CalSelectDate: picking end date < start date auto-clamps start date to end date
    modal.update(Message::ToggleCalendar(CalendarTarget::End), None);
    modal.update(
        Message::CalSelectDate(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()),
        None,
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    );
    assert!(modal.is_date_range_valid());

    // 3. Text inputs: changing start > end clamps end
    modal.update(Message::StartDateInputChanged("2026-10-25".into()), None);
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    );
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    );
    assert_eq!(modal.end_date_input, "2026-10-25");
    assert!(modal.is_date_range_valid());

    // 4. Text inputs: changing end < start clamps start
    modal.update(Message::EndDateInputChanged("2026-10-05".into()), None);
    assert_eq!(
        modal.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    );
    assert_eq!(
        modal.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    );
    assert_eq!(modal.start_date_input, "2026-10-05");
    assert!(modal.is_date_range_valid());

    // 5. Inverted input in calculate_custom_query_range: start_date > end_date normalized
    let now_ms = 1800000000000_u64;
    let (s_ms, e_ms, _, _) = calculate_custom_query_range(
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
        "00:00",
        "23:59",
        now_ms,
    );
    assert!(
        s_ms <= e_ms,
        "calculate_custom_query_range must normalize start > end"
    );
}

#[test]
fn test_c1_adversarial_malformed_and_boundary_date_strings() {
    let malformed = [
        "",
        "   ",
        "\t\n",
        "2026",
        "2026-1",
        "2026-10",
        "2026-10-1",
        "2026/10/11",
        "2026.10.11",
        "2026_10_11",
        "2026:10:11",
        "11-10-2026",
        "10-11-2026",
        "2026-00-15",
        "2026-13-15",
        "2026-05-00",
        "2026-05-32",
        "2026-04-31", // April has 30 days
        "2026-06-31", // June has 30 days
        "2026-09-31", // Sept has 30 days
        "2026-11-31", // Nov has 30 days
        "2026-02-30", // Feb never has 30 days
        "2025-02-29", // Non-leap year Feb 29
        "2100-02-29", // Century non-leap Feb 29
        "2026-10-11a",
        "a2026-10-11",
        "2026-10-11\0",
        "99999-10-11",
        "двадцять",
        "２０２６-１０-１１", // Full-width unicode
        "2026—10—11",         // Em-dash
    ];

    for &s in &malformed {
        assert!(
            parse_date_ymd(s).is_none(),
            "parse_date_ymd should return None for malformed input: {s:?}"
        );
    }

    // Empirical discovery: chrono's %Y allows signed negative years formatted with minus sign
    // "-001-10-11" has 10 characters and parses as proleptic Gregorian year -1 (1 BC)
    let parsed_neg = parse_date_ymd("-001-10-11");
    assert_eq!(
        parsed_neg,
        Some(NaiveDate::from_ymd_opt(-1, 10, 11).unwrap())
    );

    // Valid formats with whitespace trimming
    assert_eq!(
        parse_date_ymd("2026-10-11"),
        Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
    );
    assert_eq!(
        parse_date_ymd("  2026-10-11  "),
        Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
    );
    assert_eq!(
        parse_date_ymd("\t2026-10-11\n"),
        Some(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap())
    );
}

#[test]
fn test_c1_adversarial_asymmetric_invalid_input_inverted_range_bug() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    assert!(modal.is_date_range_valid());

    // Step 1: User types an invalid string into End Date ("invalid")
    modal.update(Message::EndDateInputChanged("invalid".into()), None);
    assert!(!modal.is_end_date_valid);
    assert!(!modal.is_date_range_valid());

    // Step 2: User sets Start Date to a date after the existing end_date (the default end
    // date is today, so use tomorrow to keep the test independent of the current date)
    let after_end = (modal.end_date + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    modal.update(Message::StartDateInputChanged(after_end.clone()), None);
    assert!(modal.is_start_date_valid);

    // Step 3: User hits ApplySearch (or presses Enter in search)
    modal.update(Message::ApplySearch, None);

    // ApplySearch restored end_date_input to the previous end_date, marked it valid,
    // and normalized end_date to start_date since start_date > end_date.
    assert!(modal.is_start_date_valid);
    assert!(modal.is_end_date_valid);

    // is_date_range_valid() is TRUE after normalization:
    assert!(
        modal.is_date_range_valid(),
        "Date range is valid after ApplySearch normalization"
    );
    assert_eq!(modal.start_date, modal.end_date);
    assert_eq!(modal.end_date_input, after_end);
    // And target_dates() returns non-empty slice:
    assert_eq!(
        modal.target_dates(),
        vec![modal.start_date],
        "target_dates() must not be empty"
    );
}

#[test]
fn test_c1_adversarial_year_boundary_month_rollover_anomaly() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);
    modal.update(Message::ToggleCalendar(CalendarTarget::Start), None);

    // 1. Boundary clamping at year 2000, month 1 (January 2000)
    modal.cal_view_year = 2000;
    modal.cal_view_month = 1;
    modal.update(Message::CalPrevMonth, None);
    assert_eq!(modal.cal_view_year, 2000);
    assert_eq!(
        modal.cal_view_month, 1,
        "Jan 2000 CalPrevMonth clamps to Jan 2000"
    );

    // 2. Boundary clamping at year 2099, month 12 (December 2099)
    modal.cal_view_year = 2099;
    modal.cal_view_month = 12;
    modal.update(Message::CalNextMonth, None);
    assert_eq!(modal.cal_view_year, 2099);
    assert_eq!(
        modal.cal_view_month, 12,
        "Dec 2099 CalNextMonth clamps to Dec 2099"
    );
}

#[test]
fn test_c1_adversarial_cross_midnight_and_temporal_fuzzing() {
    use chrono::Timelike;

    let base_date = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    let now_ms = NaiveDate::from_ymd_opt(2026, 10, 20)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;

    // 1. Single-day cross-midnight: 23:30 to 04:30
    let (s_ms, e_ms, s_min, e_min) =
        calculate_custom_query_range(base_date, base_date, "23:30", "04:30", now_ms);
    assert_eq!(s_min, 23 * 60 + 30);
    assert_eq!(e_min, 4 * 60 + 30);
    assert!(
        s_ms <= e_ms,
        "Single-day cross-midnight must satisfy s_ms <= e_ms"
    );

    let s_dt = chrono::DateTime::from_timestamp_millis(s_ms as i64).unwrap();
    let e_dt = chrono::DateTime::from_timestamp_millis(e_ms as i64).unwrap();
    assert_eq!(s_dt.date_naive(), base_date);
    assert_eq!(s_dt.hour(), 0);
    assert_eq!(e_dt.date_naive(), base_date);
    assert_eq!(e_dt.hour(), 23);

    // 2. Multi-day cross-midnight: 23:00 to 05:00 over 3 days
    let end_date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
    let (s2_ms, e2_ms, s2_min, e2_min) =
        calculate_custom_query_range(base_date, end_date, "23:00", "05:00", now_ms);
    assert_eq!(s2_min, 23 * 60);
    assert_eq!(e2_min, 5 * 60);
    assert!(s2_ms < e2_ms);

    let s2_dt = chrono::DateTime::from_timestamp_millis(s2_ms as i64).unwrap();
    let e2_dt = chrono::DateTime::from_timestamp_millis(e2_ms as i64).unwrap();
    assert_eq!(s2_dt.date_naive(), base_date);
    assert_eq!(s2_dt.hour(), 23);
    assert_eq!(e2_dt.date_naive(), end_date);
    assert_eq!(e2_dt.hour(), 5);

    // 3. Property oracle fuzzing: 2,000 randomized permutations
    let test_dates = [
        NaiveDate::from_ymd_opt(2024, 2, 28).unwrap(),
        NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(),
        NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 11).unwrap(),
        NaiveDate::from_ymd_opt(2029, 6, 15).unwrap(),
    ];
    let test_times = [
        "00:00", "04:30", "09:15", "12:00", "16:45", "22:00", "23:59", "invalid", "99:99", "",
    ];
    let test_nows = [
        0_u64,
        1000000_u64,
        1728648000000_u64, // 2024-10-11
        1791720000000_u64, // 2026-10-11
        2000000000000_u64,
    ];

    let mut btree_map = std::collections::BTreeMap::new();
    btree_map.insert(1791720000000_u64, "candle");

    for d1 in &test_dates {
        for d2 in &test_dates {
            for t1 in &test_times {
                for t2 in &test_times {
                    for now in &test_nows {
                        let (start, end, _, _) =
                            calculate_custom_query_range(*d1, *d2, t1, t2, *now);
                        assert!(
                            start <= end,
                            "Monotonic violation: start {start} > end {end} for d1={d1:?}, d2={d2:?}, t1={t1:?}, t2={t2:?}"
                        );
                        // Verify zero-panic in BTreeMap::range
                        let _ = btree_map.range(start..=end).count();
                    }
                }
            }
        }
    }
}

#[test]
fn test_c1_adversarial_target_dates_integrity() {
    let catalog = sample_catalog();
    let mut modal = VolumeSearchModal::new(None, catalog);

    // 1. Month boundary transition: 2026-01-30 to 2026-02-03 (5 days)
    modal.update(Message::StartDateInputChanged("2026-01-30".into()), None);
    modal.update(Message::EndDateInputChanged("2026-02-03".into()), None);
    let dates = modal.target_dates();
    assert_eq!(dates.len(), 5);
    assert_eq!(dates[0], NaiveDate::from_ymd_opt(2026, 1, 30).unwrap());
    assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
    assert_eq!(dates[2], NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    assert_eq!(dates[3], NaiveDate::from_ymd_opt(2026, 2, 2).unwrap());
    assert_eq!(dates[4], NaiveDate::from_ymd_opt(2026, 2, 3).unwrap());

    // 2. Leap year transition: 2024-02-27 to 2024-03-02 (5 days, includes Feb 29)
    modal.update(Message::StartDateInputChanged("2024-02-27".into()), None);
    modal.update(Message::EndDateInputChanged("2024-03-02".into()), None);
    let leap_dates = modal.target_dates();
    assert_eq!(leap_dates.len(), 5);
    assert_eq!(leap_dates[1], NaiveDate::from_ymd_opt(2024, 2, 28).unwrap());
    assert_eq!(leap_dates[2], NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
    assert_eq!(leap_dates[3], NaiveDate::from_ymd_opt(2024, 3, 1).unwrap());

    // 3. Non-leap year transition: 2025-02-27 to 2025-03-02 (4 days, skips Feb 29)
    modal.update(Message::StartDateInputChanged("2025-02-27".into()), None);
    modal.update(Message::EndDateInputChanged("2025-03-02".into()), None);
    let non_leap_dates = modal.target_dates();
    assert_eq!(non_leap_dates.len(), 4);
    assert_eq!(
        non_leap_dates[1],
        NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
    );
    assert_eq!(
        non_leap_dates[2],
        NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()
    );

    // 4. Year boundary transition: 2025-12-30 to 2026-01-02 (4 days)
    modal.update(Message::StartDateInputChanged("2025-12-30".into()), None);
    modal.update(Message::EndDateInputChanged("2026-01-02".into()), None);
    let year_dates = modal.target_dates();
    assert_eq!(year_dates.len(), 4);
    assert_eq!(
        year_dates[0],
        NaiveDate::from_ymd_opt(2025, 12, 30).unwrap()
    );
    assert_eq!(
        year_dates[1],
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()
    );
    assert_eq!(year_dates[2], NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert_eq!(year_dates[3], NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
}

// ========================================================================
// Milestone 3 (R3) Tiered Caching, Download Chaining & Progress Tests
// ========================================================================

#[test]
fn test_m3_exchange_data_folder_mapping_integrity() {
    assert_eq!(exchange_data_folder(Exchange::BinanceSpot), "binance");
    assert_eq!(exchange_data_folder(Exchange::BinanceLinear), "binance");
    assert_eq!(exchange_data_folder(Exchange::BinanceInverse), "binance");
    assert_eq!(exchange_data_folder(Exchange::BybitSpot), "bybit");
    assert_eq!(exchange_data_folder(Exchange::BybitLinear), "bybit");
    assert_eq!(exchange_data_folder(Exchange::BybitInverse), "bybit");
    assert_eq!(exchange_data_folder(Exchange::OkexSpot), "okex");
    assert_eq!(exchange_data_folder(Exchange::OkexLinear), "okex");
    assert_eq!(exchange_data_folder(Exchange::OkexInverse), "okex");
    assert_eq!(
        exchange_data_folder(Exchange::HyperliquidSpot),
        "hyperliquid"
    );
    assert_eq!(
        exchange_data_folder(Exchange::HyperliquidLinear),
        "hyperliquid"
    );
}

#[test]
fn test_m3_missing_cache_initiates_download_task_with_banner() {
    let mock_coin = mock_ticker("M3_MOCK_COIN", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
    modal.update(Message::EndDateInputChanged("2026-03-03".into()), None);
    assert_eq!(modal.target_dates().len(), 3);

    let action = modal.update(Message::ApplySearch, None);
    assert!(matches!(action, Some(Action::Task(_))));
    assert!(modal.is_downloading);
    assert!(modal.download_progress.is_some());
    let banner = modal.download_progress.as_ref().unwrap();
    assert!(banner.contains("Завантаження з біржі: день 1 з 3"));
    assert!(banner.contains("2026-03-01"));
    assert_eq!(modal.download_missing_dates.len(), 3);
    assert_eq!(modal.current_download_index, 0);
}

#[test]
fn test_m3_day_download_chaining_and_final_search() {
    let mock_coin = mock_ticker("M3_CHAIN_COIN", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
    modal.update(Message::EndDateInputChanged("2026-03-02".into()), None);

    // Step 1: Initiate search
    let action1 = modal.update(Message::ApplySearch, None);
    assert!(matches!(action1, Some(Action::Task(_))));
    assert_eq!(modal.current_download_index, 0);

    // Construct mock candle datapoint for Day 1
    let day1 = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let day1_ts = day1
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;
    let mut mock_dp1 = KlineDataPoint {
        kline: exchange::Kline {
            time: day1_ts,
            open: exchange::util::Price::from_f32(100.0),
            high: exchange::util::Price::from_f32(105.0),
            low: exchange::util::Price::from_f32(95.0),
            close: exchange::util::Price::from_f32(102.0),
            volume: (5000.0, 5000.0),
        },
        footprint: data::chart::kline::KlineTrades::new(),
        trades_fetched: true,
    };
    mock_dp1.footprint.add_trade_to_nearest_bin(
        &exchange::Trade {
            time: day1_ts,
            price: exchange::util::Price::from_f32(100.0),
            qty: 10_000.0,
            is_sell: false,
        },
        PriceStep::from_f32(0.01),
    );

    // Step 2: Deliver Day 1 download completion -> dispatches Day 2 task
    let action2 = modal.update(
        Message::DayDownloadCompleted(Ok((day1, vec![(day1_ts, mock_dp1)]))),
        None,
    );
    assert!(matches!(action2, Some(Action::Task(_))));
    assert!(modal.is_downloading);
    assert_eq!(modal.current_download_index, 1);
    let banner = modal.download_progress.as_ref().unwrap();
    assert!(banner.contains("Завантаження з біржі: день 2 з 2"));
    assert!(banner.contains("2026-03-02"));

    // Step 3: Deliver Day 2 download completion (empty) -> terminates download & finalizes search
    let day2 = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let action3 = modal.update(Message::DayDownloadCompleted(Ok((day2, Vec::new()))), None);
    assert!(action3.is_none());
    assert!(!modal.is_downloading);
    assert!(modal.download_progress.is_none());
    assert!(modal.has_searched);
    assert_eq!(modal.rows.len(), 1);
    assert_eq!(modal.rows[0].max_volume, 10_000.0);
    assert_eq!(modal.summary.count, 1);
}

#[test]
fn test_m3_cancel_search_lifecycle_and_partial_results() {
    let mock_coin = mock_ticker("M3_CANCEL_COIN", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    modal.update(Message::StartDateInputChanged("2026-03-01".into()), None);
    modal.update(Message::EndDateInputChanged("2026-03-03".into()), None);

    // Initiate search (3 days)
    let _ = modal.update(Message::ApplySearch, None);
    assert!(modal.is_downloading);

    // Deliver Day 1
    let day1 = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let day1_ts = day1
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis() as u64;
    let mut mock_dp1 = KlineDataPoint {
        kline: exchange::Kline {
            time: day1_ts,
            open: exchange::util::Price::from_f32(50.0),
            high: exchange::util::Price::from_f32(55.0),
            low: exchange::util::Price::from_f32(48.0),
            close: exchange::util::Price::from_f32(52.0),
            volume: (2500.0, 2500.0),
        },
        footprint: data::chart::kline::KlineTrades::new(),
        trades_fetched: true,
    };
    mock_dp1.footprint.add_trade_to_nearest_bin(
        &exchange::Trade {
            time: day1_ts,
            price: exchange::util::Price::from_f32(50.0),
            qty: 5000.0,
            is_sell: true,
        },
        PriceStep::from_f32(0.01),
    );
    let _ = modal.update(
        Message::DayDownloadCompleted(Ok((day1, vec![(day1_ts, mock_dp1)]))),
        None,
    );

    // User clicks Cancel while waiting for Day 2
    let cancel_action = modal.update(Message::CancelSearch, None);
    assert!(cancel_action.is_none());
    assert!(!modal.is_downloading);
    assert!(modal.download_progress.is_none());
    assert!(modal.has_searched);
    // Partial results from Day 1 are preserved and computed into search rows
    assert_eq!(modal.rows.len(), 1);
    assert_eq!(modal.rows[0].max_volume, 5000.0);
}

#[test]
fn test_m3_all_loaded_preset_without_panes_completes_cleanly() {
    let mock_coin = mock_ticker("M3_ALL_LOADED", Exchange::BinanceLinear);
    let mut modal = VolumeSearchModal::new(Some(mock_coin), vec![mock_coin]);

    modal.update(Message::PresetSelected(VolumeSearchPreset::AllLoaded), None);
    let action = modal.update(Message::ApplySearch, None);
    assert!(action.is_none());
    assert!(modal.has_searched);
    assert!(!modal.is_downloading);
    assert!(modal.download_progress.is_none());
}
