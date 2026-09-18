use hawk_terminal::data::chart::ViewMode;
use hawk_terminal::data::chart::kline::{KlineChartKind, TpoColorScheme, TpoElementColor};
use hawk_terminal::exchange::Kline;
use hawk_terminal::exchange::util::Price;
use hawk_terminal::profile::session::{
    SessionCluster, SessionPeriod, apply_session_clusters, group_candles_by_period,
    merge_adjacent_clusters, period_bounds_utc, split_cluster,
};
use hawk_terminal::profile::tpo::{
    build_tpo_profile, get_tpo_bracket_continuous, merge_tpo_profiles,
};

fn make_kline(open_time: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Kline {
    Kline {
        time: open_time as u64,
        open: Price::from_f32(open as f32),
        high: Price::from_f32(high as f32),
        low: Price::from_f32(low as f32),
        close: Price::from_f32(close as f32),
        volume: (volume as f32 * 0.5, volume as f32 * 0.5),
    }
}

#[test]
fn test_multi_period_bounds_and_grouping() {
    // Mon, 15 Jan 2024 00:00:00 GMT = 1705276800000
    let mon_start = 1705276800000i64;
    let wed_time = mon_start + 2 * 86_400_000 + 3_600_000; // Wed 01:00 UTC
    let next_mon_time = mon_start + 7 * 86_400_000 + 1_000;

    // 1. Daily
    let (d_start, d_end) = period_bounds_utc(wed_time, SessionPeriod::Daily);
    assert_eq!(d_end - d_start, 86_400_000);
    assert_eq!(d_start, mon_start + 2 * 86_400_000);

    // 2. Weekly
    let (w_start, w_end) = period_bounds_utc(wed_time, SessionPeriod::Weekly);
    assert_eq!(w_start, mon_start);
    assert_eq!(w_end, mon_start + 7 * 86_400_000);

    let (w2_start, _) = period_bounds_utc(next_mon_time, SessionPeriod::Weekly);
    assert_eq!(w2_start, mon_start + 7 * 86_400_000);

    // 3. Monthly
    let (m_start, m_end) = period_bounds_utc(wed_time, SessionPeriod::Monthly);
    // Jan 1 2024 00:00:00 UTC = 1704067200000
    // Feb 1 2024 00:00:00 UTC = 1706745600000
    assert_eq!(m_start, 1704067200000);
    assert_eq!(m_end, 1706745600000);

    // 4. Custom N-Days (4 days)
    let (c_start, c_end) = period_bounds_utc(mon_start + 10_000, SessionPeriod::CustomDays(4));
    assert_eq!(c_end - c_start, 4 * 86_400_000);

    // Grouping verification across 14 consecutive days
    let mut candles = Vec::new();
    for day in 0..14 {
        for half_hour in 0..48 {
            let t = mon_start + (day * 86_400_000) + (half_hour * 1_800_000);
            candles.push(make_kline(t, 100.0, 105.0, 95.0, 100.0, 10.0));
        }
    }

    let daily_sessions = group_candles_by_period(&candles, SessionPeriod::Daily);
    assert_eq!(daily_sessions.len(), 14);

    let weekly_sessions = group_candles_by_period(&candles, SessionPeriod::Weekly);
    assert_eq!(weekly_sessions.len(), 2);
    assert_eq!(weekly_sessions[0].2.len(), 7 * 48);
    assert_eq!(weekly_sessions[1].2.len(), 7 * 48);

    let custom4_sessions = group_candles_by_period(&candles, SessionPeriod::CustomDays(4));
    assert!(custom4_sessions.len() >= 3 && custom4_sessions.len() <= 5);
}

#[test]
fn test_continuous_bracket_cycling_beyond_52() {
    let start = 1705276800000i64; // Mon 00:00 UTC
    let half_hour = 1_800_000i64;

    // Slot 0: 'A'
    assert_eq!(get_tpo_bracket_continuous(start, start), Some('A'));
    // Slot 25: 'Z'
    assert_eq!(
        get_tpo_bracket_continuous(start + 25 * half_hour, start),
        Some('Z')
    );
    // Slot 26: 'a'
    assert_eq!(
        get_tpo_bracket_continuous(start + 26 * half_hour, start),
        Some('a')
    );
    // Slot 51: 'z'
    assert_eq!(
        get_tpo_bracket_continuous(start + 51 * half_hour, start),
        Some('z')
    );
    // Slot 52: 'A' (cycle repeat)
    assert_eq!(
        get_tpo_bracket_continuous(start + 52 * half_hour, start),
        Some('A')
    );
    // Slot 78: 'a' (cycle 1 lowercase repeat)
    assert_eq!(
        get_tpo_bracket_continuous(start + 78 * half_hour, start),
        Some('a')
    );
    // Slot 104: 'A' (2nd cycle repeat)
    assert_eq!(
        get_tpo_bracket_continuous(start + 104 * half_hour, start),
        Some('A')
    );

    // Verify building TPO profile for 7-day session (336 slots) doesn't panic and populates continuous brackets
    let mut candles = Vec::new();
    for slot in 0..100 {
        let t = start + slot * half_hour;
        candles.push(make_kline(t, 200.0, 202.0, 198.0, 200.0, 1.0));
    }

    let profile = build_tpo_profile(&candles, start, start + 7 * 86_400_000, "Week", 1.0);
    assert!(!profile.matrix.is_empty());
    // Check that row has brackets beyond standard 52 letters
    let mid_row = &profile.matrix[&200];
    assert!(mid_row.len() >= 52);
    assert!(mid_row.contains(&'A'));
    assert!(mid_row.contains(&'Z'));
    assert!(mid_row.contains(&'a'));
    assert!(mid_row.contains(&'z'));
}

#[test]
fn test_initial_balance_lock_to_opening_intervals() {
    let start = 1705276800000i64; // Mon 00:00 UTC
    let half_hour = 1_800_000i64;

    let candles = vec![
        // First hour (brackets 0 and 1): price between 100.0 and 110.0
        make_kline(start, 100.0, 105.0, 100.0, 104.0, 10.0),
        make_kline(start + half_hour, 104.0, 110.0, 102.0, 108.0, 10.0),
        // Later in session (day 3): extreme spike to 150.0 and dip to 50.0
        make_kline(start + 50 * half_hour, 110.0, 150.0, 50.0, 120.0, 10.0),
    ];

    let profile = build_tpo_profile(&candles, start, start + 7 * 86_400_000, "Extended", 1.0);
    let ib = profile
        .ib
        .as_ref()
        .expect("Initial Balance must be present");

    // IB must be strictly locked to opening hour: low 100.0, high 110.0
    assert!((ib.low - 100.0).abs() < 1e-4);
    assert!((ib.high - 110.0).abs() < 1e-4);
    assert!((ib.extension_1_5 - (110.0 + 10.0 * 0.5)).abs() < 1e-4);
    assert!((ib.extension_2_0 - (110.0 + 10.0 * 1.0)).abs() < 1e-4);

    // Verify Initial Balance recalculation across merged sessions (Day 1 + Day 2)
    let d2_start = start + 86_400_000;
    let d2_candles = vec![
        make_kline(d2_start, 180.0, 190.0, 175.0, 185.0, 10.0),
        make_kline(d2_start + half_hour, 185.0, 195.0, 180.0, 190.0, 10.0),
    ];
    let p2 = build_tpo_profile(&d2_candles, d2_start, d2_start + 86_400_000, "Day2", 1.0);
    let merged = merge_tpo_profiles(&[profile.clone(), p2]).expect("Merged profile");
    let merged_ib = merged
        .ib
        .expect("Merged profile must calculate composite IB");
    assert!(
        (merged_ib.low - 100.0).abs() < 1e-4,
        "Merged IB min bound across 'A' and 'B' brackets"
    );
    assert!(
        (merged_ib.high - 195.0).abs() < 1e-4,
        "Merged IB max bound across 'A' and 'B' brackets"
    );
}

#[test]
fn test_selective_cluster_merging_and_adjacent_isolation() {
    let base_start = 1705276800000i64;
    let day_ms = 86_400_000i64;

    // 4 sessions: s0, s1, s2, s3
    let s0 = base_start;
    let s1 = base_start + day_ms;
    let s2 = base_start + 2 * day_ms;
    let s3 = base_start + 3 * day_ms;

    let mut candles = Vec::new();
    for (i, &s) in [s0, s1, s2, s3].iter().enumerate() {
        let price = 100.0 + (i as f64 * 10.0);
        candles.push(make_kline(s, price, price + 5.0, price - 5.0, price, 100.0));
    }

    let sessions = group_candles_by_period(&candles, SessionPeriod::Daily);
    assert_eq!(sessions.len(), 4);

    let raw_profiles: Vec<_> = sessions
        .iter()
        .map(|(start, end, sc)| build_tpo_profile(sc, *start, *end, "Day", 1.0))
        .collect();
    assert_eq!(raw_profiles.len(), 4);

    // Initial state: no clusters -> 4 distinct profiles
    let mut clusters: Vec<SessionCluster> = Vec::new();
    let merged_tpos = apply_session_clusters(&raw_profiles, &clusters);
    assert_eq!(merged_tpos.len(), 4);

    // Merge s1 and s2 selectively
    merge_adjacent_clusters(&mut clusters, s1, s2);
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].session_starts, vec![s1, s2]);

    let merged_tpos = apply_session_clusters(&raw_profiles, &clusters);
    // Should now produce 3 profiles: s0, (s1+s2), s3
    assert_eq!(merged_tpos.len(), 3);

    // s0 is unmerged
    assert_eq!(merged_tpos[0].session_start, s0);
    assert_eq!(merged_tpos[0].session_end, s0 + day_ms);

    // Cluster (s1+s2) covers s1 start to s2 end
    assert_eq!(merged_tpos[1].session_start, s1);
    assert_eq!(merged_tpos[1].session_end, s2 + day_ms);

    // s3 is unmerged
    assert_eq!(merged_tpos[2].session_start, s3);
    assert_eq!(merged_tpos[2].session_end, s3 + day_ms);

    // Further merge s3 into the cluster: s2 and s3
    merge_adjacent_clusters(&mut clusters, s2, s3);
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].session_starts, vec![s1, s2, s3]);

    let merged_tpos_3 = apply_session_clusters(&raw_profiles, &clusters);
    assert_eq!(merged_tpos_3.len(), 2);
    assert_eq!(merged_tpos_3[0].session_start, s0);
    assert_eq!(merged_tpos_3[1].session_start, s1);
    assert_eq!(merged_tpos_3[1].session_end, s3 + day_ms);

    // Split cluster by s2
    split_cluster(&mut clusters, s2);
    assert!(clusters.is_empty());

    let split_tpos = apply_session_clusters(&raw_profiles, &clusters);
    assert_eq!(split_tpos.len(), 4);
    assert_eq!(split_tpos[0].session_start, s0);
    assert_eq!(split_tpos[1].session_start, s1);
    assert_eq!(split_tpos[2].session_start, s2);
    assert_eq!(split_tpos[3].session_start, s3);
}

#[test]
fn test_display_mode_isolation_and_enforcement() {
    // 1. Combined View Mode: show_candles == true
    // Honors user-selected aggregation period
    let combined_kind = KlineChartKind::Tpo {
        show_candles: true,
        show_letters: true,
        show_ib: true,
        show_va: true,
        show_poc: true,
        show_single_prints: true,
        tick_step: Default::default(),
        period: SessionPeriod::Weekly,
        clusters: Vec::new(),
        split_sessions: Vec::new(),
        color_scheme: Default::default(),
        ib_color: Default::default(),
        poc_color: Default::default(),
        single_prints_color: Default::default(),
    };
    assert_eq!(combined_kind.view_mode(), ViewMode::Combined);
    assert_eq!(
        combined_kind.effective_tpo_period(),
        SessionPeriod::Weekly,
        "Combined mode honors user-selected aggregation period"
    );

    // 2. Pure TPO View Mode: show_candles == false
    // MUST honor the user-selected period (e.g. Weekly, Monthly, CustomDays(4))
    let pure_weekly = KlineChartKind::Tpo {
        show_candles: false,
        show_letters: true,
        show_ib: true,
        show_va: true,
        show_poc: true,
        show_single_prints: true,
        tick_step: Default::default(),
        period: SessionPeriod::Weekly,
        clusters: Vec::new(),
        split_sessions: Vec::new(),
        color_scheme: Default::default(),
        ib_color: Default::default(),
        poc_color: Default::default(),
        single_prints_color: Default::default(),
    };
    assert_eq!(pure_weekly.view_mode(), ViewMode::TpoOnly);
    assert_eq!(
        pure_weekly.effective_tpo_period(),
        SessionPeriod::Weekly,
        "Pure TPO mode MUST honor Weekly aggregation"
    );

    let pure_custom4 = KlineChartKind::Tpo {
        show_candles: false,
        show_letters: true,
        show_ib: true,
        show_va: true,
        show_poc: true,
        show_single_prints: true,
        tick_step: Default::default(),
        period: SessionPeriod::CustomDays(4),
        clusters: Vec::new(),
        split_sessions: Vec::new(),
        color_scheme: Default::default(),
        ib_color: Default::default(),
        poc_color: Default::default(),
        single_prints_color: Default::default(),
    };
    assert_eq!(
        pure_custom4.effective_tpo_period(),
        SessionPeriod::CustomDays(4)
    );
}

#[test]
fn test_workspace_and_pane_config_persistence_roundtrip() {
    let tpo_kind = KlineChartKind::Tpo {
        show_candles: false,
        show_letters: true,
        show_ib: true,
        show_va: true,
        show_poc: true,
        show_single_prints: true,
        tick_step: Default::default(),
        period: SessionPeriod::CustomDays(4),
        clusters: vec![
            SessionCluster::new(vec![1705276800000, 1705363200000]),
            SessionCluster::new(vec![1705536000000, 1705622400000, 1705708800000]),
        ],
        split_sessions: vec![1705276800000],
        color_scheme: Default::default(),
        ib_color: Default::default(),
        poc_color: Default::default(),
        single_prints_color: Default::default(),
    };

    let json = serde_json::to_string_pretty(&tpo_kind).expect("Serialize KlineChartKind");
    assert!(json.contains("CustomDays"));
    assert!(json.contains("1705276800000"));

    let loaded: KlineChartKind = serde_json::from_str(&json).expect("Deserialize KlineChartKind");
    if let KlineChartKind::Tpo {
        period,
        clusters,
        split_sessions,
        ..
    } = loaded
    {
        assert_eq!(period, SessionPeriod::CustomDays(4));
        assert_eq!(clusters.len(), 2);
        assert_eq!(
            clusters[0].session_starts,
            vec![1705276800000, 1705363200000]
        );
        assert_eq!(
            clusters[1].session_starts,
            vec![1705536000000, 1705622400000, 1705708800000]
        );
        assert_eq!(split_sessions, vec![1705276800000]);
    } else {
        panic!("Expected KlineChartKind::Tpo");
    }

    // Backward compatibility: JSON without period, clusters, and split_sessions defaults properly
    let legacy_json = r#"{"Tpo":{"show_candles":true,"show_letters":true,"show_ib":true,"show_va":true,"show_poc":true,"show_single_prints":true,"tick_step":"Auto"}}"#;
    let legacy_loaded: KlineChartKind =
        serde_json::from_str(legacy_json).expect("Deserialize legacy KlineChartKind");
    if let KlineChartKind::Tpo {
        period,
        clusters,
        split_sessions,
        ..
    } = legacy_loaded
    {
        assert_eq!(period, SessionPeriod::Daily);
        assert!(clusters.is_empty());
        assert!(split_sessions.is_empty());
    } else {
        panic!("Expected KlineChartKind::Tpo");
    }
}

#[test]
fn test_view_mode_pure_tpo_alias() {
    assert_eq!(ViewMode::PureTpo, ViewMode::TpoOnly);
    assert_eq!(ViewMode::PURE_TPO, ViewMode::TpoOnly);
    assert!(ViewMode::PureTpo.is_pure_tpo());
    assert!(!ViewMode::Combined.is_pure_tpo());
    assert!(!ViewMode::CandlesOnly.is_pure_tpo());

    // Serde alias verification
    let json_pure = "\"PureTpo\"";
    let deserialized: ViewMode = serde_json::from_str(json_pure).expect("Deserialize PureTpo");
    assert_eq!(deserialized, ViewMode::TpoOnly);
}

#[test]
fn test_trading_sessions_partitioning() {
    let day_start = 1705276800000i64; // Mon 00:00:00 UTC
    let hour = 3_600_000i64;

    // 1. Asia session (00:00 - 08:00 UTC)
    let (s_asia, e_asia) = period_bounds_utc(day_start + 2 * hour, SessionPeriod::TradingSessions);
    assert_eq!(s_asia, day_start);
    assert_eq!(e_asia, day_start + 8 * hour);

    // 2. London session (08:00 - 16:00 UTC)
    let (s_ldn, e_ldn) = period_bounds_utc(day_start + 10 * hour, SessionPeriod::TradingSessions);
    assert_eq!(s_ldn, day_start + 8 * hour);
    assert_eq!(e_ldn, day_start + 16 * hour);

    // 3. New York session (16:00 - 24:00 UTC)
    let (s_ny, e_ny) = period_bounds_utc(day_start + 20 * hour, SessionPeriod::TradingSessions);
    assert_eq!(s_ny, day_start + 16 * hour);
    assert_eq!(e_ny, day_start + 24 * hour);

    // Grouping verification for 24h into 3 sessions
    let mut candles = Vec::new();
    for h in 0..24 {
        candles.push(make_kline(
            day_start + h * hour,
            100.0,
            105.0,
            95.0,
            100.0,
            10.0,
        ));
    }
    let sessions = group_candles_by_period(&candles, SessionPeriod::TradingSessions);
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].0, day_start);
    assert_eq!(sessions[0].1, day_start + 8 * hour);
    assert_eq!(sessions[0].2.len(), 8);
    assert_eq!(sessions[1].0, day_start + 8 * hour);
    assert_eq!(sessions[1].1, day_start + 16 * hour);
    assert_eq!(sessions[1].2.len(), 8);
    assert_eq!(sessions[2].0, day_start + 16 * hour);
    assert_eq!(sessions[2].1, day_start + 24 * hour);
    assert_eq!(sessions[2].2.len(), 8);
}

#[test]
fn test_bracket_indices_chronological_integrity() {
    let start = 1705276800000i64; // Mon 00:00 UTC
    let half_hour = 1_800_000i64;

    let mut candles = Vec::new();
    for slot in 0..100 {
        let t = start + slot * half_hour;
        candles.push(make_kline(t, 200.0, 202.0, 198.0, 200.0, 1.0));
    }

    let profile = build_tpo_profile(&candles, start, start + 7 * 86_400_000, "Week", 1.0);
    assert!(!profile.bracket_indices.is_empty());
    let indices = &profile.bracket_indices[&200];
    assert_eq!(indices.len(), 100);
    assert_eq!(indices[0], 0);
    assert_eq!(indices[51], 51);
    assert_eq!(indices[52], 52); // Index beyond 51 is preserved, not wrapped!
    assert_eq!(indices[99], 99);
}

#[test]
fn test_merged_clusters_bracket_indices() {
    let start1 = 1705276800000i64; // Day 1
    let day_ms = 86_400_000i64;
    let start2 = start1 + day_ms; // Day 2

    let c1 = vec![make_kline(start1, 100.0, 105.0, 95.0, 100.0, 1.0)];
    let p1 = build_tpo_profile(&c1, start1, start1 + day_ms, "Day1", 1.0);

    let c2 = vec![make_kline(start2, 100.0, 105.0, 95.0, 100.0, 1.0)];
    let p2 = build_tpo_profile(&c2, start2, start2 + day_ms, "Day2", 1.0);

    let merged = merge_tpo_profiles(&[p1, p2]).expect("Merged profile");
    let indices = &merged.bracket_indices[&100];
    assert!(indices.contains(&0)); // Day 1 bracket 0
    assert!(indices.contains(&48)); // Day 2 bracket 0 is offset by 48 (24h)!
}

#[test]
fn test_tpo_color_scheme_serde_and_palette() {
    let default_scheme: TpoColorScheme =
        serde_json::from_str("\"Classic\"").expect("Classic scheme");
    assert_eq!(default_scheme, TpoColorScheme::Classic);

    let theme_scheme: TpoColorScheme = serde_json::from_str("\"Theme\"").expect("Theme scheme");
    assert_eq!(theme_scheme, TpoColorScheme::Theme);

    let custom_scheme = TpoColorScheme::Custom([255, 128, 64]);
    let serialized = serde_json::to_string(&custom_scheme).expect("Serialize Custom");
    let deserialized: TpoColorScheme =
        serde_json::from_str(&serialized).expect("Deserialize Custom");
    assert_eq!(deserialized, custom_scheme);
    assert_eq!(custom_scheme.custom_rgb(), Some([255, 128, 64]));
}

#[test]
fn test_tpo_element_colors_serde_and_resolution() {
    let auto_col: TpoElementColor = serde_json::from_str("\"Auto\"").expect("Auto element color");
    assert_eq!(auto_col, TpoElementColor::Auto);
    assert_eq!(auto_col, TpoElementColor::default());

    let theme_col: TpoElementColor =
        serde_json::from_str("\"Theme\"").expect("Theme element color");
    assert_eq!(theme_col, TpoElementColor::Theme);

    let red_col = TpoElementColor::Red;
    assert_eq!(red_col.to_rgb(), Some([235, 75, 75]));

    let custom_col = TpoElementColor::Custom([10, 200, 150]);
    let serialized = serde_json::to_string(&custom_col).expect("Serialize Custom element color");
    let deserialized: TpoElementColor =
        serde_json::from_str(&serialized).expect("Deserialize Custom element color");
    assert_eq!(deserialized, custom_col);
    assert_eq!(custom_col.custom_rgb(), Some([10, 200, 150]));
}
