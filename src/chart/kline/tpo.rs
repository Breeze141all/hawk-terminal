//! TPO / market profile drawing and its context menu.
//!
//! Split out of `chart/kline.rs` (ROADMAP #131); the code is unchanged apart from
//! `pub(super)` visibility.

use super::*;

pub(super) fn bracket_color(
    bracket: char,
    scheme: data::chart::kline::TpoColorScheme,
    palette: &Extended,
) -> iced::Color {
    let idx = if bracket.is_ascii_uppercase() {
        (bracket as u8 - b'A') as usize
    } else if bracket.is_ascii_lowercase() {
        (bracket as u8 - b'a' + 26) as usize
    } else {
        0
    };

    match scheme {
        data::chart::kline::TpoColorScheme::Classic => {
            const PALETTE: [iced::Color; 8] = [
                iced::Color::from_rgb(0.20, 0.65, 0.95), // Sky Blue (0h-2h)
                iced::Color::from_rgb(0.12, 0.76, 0.80), // Teal/Cyan (2h-4h)
                iced::Color::from_rgb(0.22, 0.76, 0.54), // Emerald Mint (4h-6h)
                iced::Color::from_rgb(0.98, 0.74, 0.18), // Golden Amber (6h-8h)
                iced::Color::from_rgb(0.98, 0.48, 0.22), // Sunset Orange (8h-10h)
                iced::Color::from_rgb(0.92, 0.32, 0.45), // Coral Rose (10h-12h)
                iced::Color::from_rgb(0.68, 0.36, 0.82), // Violet Purple (12h-14h)
                iced::Color::from_rgb(0.42, 0.45, 0.86), // Royal Indigo (14h-16h)
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Theme => {
            let colors = [
                palette.primary.base.color,
                palette.success.base.color,
                palette.warning.base.color,
                palette.danger.base.color,
                palette.primary.strong.color,
                palette.success.weak.color,
                palette.warning.weak.color,
                palette.danger.weak.color,
            ];
            colors[(idx / 4) % colors.len()]
        }
        data::chart::kline::TpoColorScheme::Cyan => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.12, 0.85, 0.95),
                iced::Color::from_rgb(0.08, 0.75, 0.88),
                iced::Color::from_rgb(0.20, 0.65, 0.90),
                iced::Color::from_rgb(0.00, 0.90, 0.80),
                iced::Color::from_rgb(0.30, 0.70, 0.98),
                iced::Color::from_rgb(0.15, 0.60, 0.85),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Emerald => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.20, 0.85, 0.55),
                iced::Color::from_rgb(0.15, 0.75, 0.45),
                iced::Color::from_rgb(0.10, 0.90, 0.65),
                iced::Color::from_rgb(0.30, 0.80, 0.40),
                iced::Color::from_rgb(0.25, 0.70, 0.50),
                iced::Color::from_rgb(0.05, 0.65, 0.35),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Amber => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.98, 0.78, 0.18),
                iced::Color::from_rgb(0.95, 0.65, 0.15),
                iced::Color::from_rgb(0.98, 0.55, 0.20),
                iced::Color::from_rgb(0.90, 0.70, 0.25),
                iced::Color::from_rgb(0.85, 0.60, 0.10),
                iced::Color::from_rgb(0.99, 0.82, 0.30),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Purple => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.72, 0.40, 0.92),
                iced::Color::from_rgb(0.60, 0.30, 0.85),
                iced::Color::from_rgb(0.80, 0.45, 0.98),
                iced::Color::from_rgb(0.55, 0.35, 0.80),
                iced::Color::from_rgb(0.85, 0.50, 0.90),
                iced::Color::from_rgb(0.50, 0.25, 0.75),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Red => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.95, 0.30, 0.40),
                iced::Color::from_rgb(0.85, 0.25, 0.35),
                iced::Color::from_rgb(0.98, 0.40, 0.48),
                iced::Color::from_rgb(0.80, 0.20, 0.30),
                iced::Color::from_rgb(0.90, 0.35, 0.42),
                iced::Color::from_rgb(0.75, 0.15, 0.25),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Blue => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.25, 0.60, 0.98),
                iced::Color::from_rgb(0.20, 0.50, 0.90),
                iced::Color::from_rgb(0.35, 0.70, 0.99),
                iced::Color::from_rgb(0.15, 0.45, 0.85),
                iced::Color::from_rgb(0.30, 0.65, 0.95),
                iced::Color::from_rgb(0.10, 0.40, 0.80),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Monochrome => {
            const PALETTE: [iced::Color; 6] = [
                iced::Color::from_rgb(0.85, 0.85, 0.85),
                iced::Color::from_rgb(0.72, 0.72, 0.72),
                iced::Color::from_rgb(0.92, 0.92, 0.92),
                iced::Color::from_rgb(0.65, 0.65, 0.65),
                iced::Color::from_rgb(0.78, 0.78, 0.78),
                iced::Color::from_rgb(0.58, 0.58, 0.58),
            ];
            PALETTE[(idx / 4) % PALETTE.len()]
        }
        data::chart::kline::TpoColorScheme::Custom([r, g, b]) => {
            let base_r = r as f32 / 255.0;
            let base_g = g as f32 / 255.0;
            let base_b = b as f32 / 255.0;
            let factors = [1.0, 0.85, 1.15, 0.75, 1.05, 0.90];
            let f = factors[(idx / 4) % factors.len()];
            iced::Color::from_rgb(
                (base_r * f).min(1.0),
                (base_g * f).min(1.0),
                (base_b * f).min(1.0),
            )
        }
    }
}

pub(super) fn draw_context_menu(
    frame: &mut canvas::Frame,
    menu: &TpoContextMenu,
    cursor_pos: Option<Point>,
    palette: &Extended,
) {
    // 1. Soft layered drop shadow
    frame.fill_rectangle(
        Point::new(menu.rect.x - 2.0, menu.rect.y + 2.0),
        Size::new(menu.rect.width + 4.0, menu.rect.height + 4.0),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.12),
    );
    frame.fill_rectangle(
        Point::new(menu.rect.x - 1.0, menu.rect.y + 1.0),
        Size::new(menu.rect.width + 2.0, menu.rect.height + 2.0),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.20),
    );
    frame.fill_rectangle(
        Point::new(menu.rect.x + 1.0, menu.rect.y + 2.0),
        menu.rect.size(),
        iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
    );

    // 2. Menu body & border in theme palette
    let bg_color = palette.background.base.color;
    let border_color = palette.background.weak.color;

    frame.fill_rectangle(menu.rect.position(), menu.rect.size(), bg_color);
    frame.stroke(
        &Path::rectangle(menu.rect.position(), menu.rect.size()),
        Stroke::with_color(
            Stroke {
                width: 1.0,
                ..Default::default()
            },
            border_color,
        ),
    );

    // 3. Typography & item states
    let text_active = palette.background.base.text;
    let text_disabled = palette.background.base.text.scale_alpha(0.35);
    let hover_bg = if palette.is_dark {
        palette.background.weak.color
    } else {
        palette.background.strong.color
    };
    let divider_color = palette.background.weak.color.scale_alpha(0.45);

    for (i, item) in menu.items.iter().enumerate() {
        let is_enabled = item.action.is_some();
        let is_hovered = cursor_pos.is_some_and(|cp| item.rect.contains(cp));

        if is_enabled && is_hovered {
            let hover_rect = Rectangle::new(
                Point::new(item.rect.x + 3.0, item.rect.y + 1.0),
                Size::new(item.rect.width - 6.0, item.rect.height - 2.0),
            );
            frame.fill_rectangle(hover_rect.position(), hover_rect.size(), hover_bg);
        }

        let color = if is_enabled {
            text_active
        } else {
            text_disabled
        };

        frame.fill_text(canvas::Text {
            content: item.label.clone(),
            position: Point::new(item.rect.x + 14.0, item.rect.y + item.rect.height / 2.0),
            size: iced::Pixels(12.0),
            color,
            align_y: Alignment::Center.into(),
            align_x: Alignment::Start.into(),
            ..canvas::Text::default()
        });

        if i + 1 < menu.items.len() {
            let line_y = item.rect.y + item.rect.height;
            frame.fill_rectangle(
                Point::new(menu.rect.x + 8.0, line_y),
                Size::new(menu.rect.width - 16.0, 1.0),
                divider_color,
            );
        }
    }
}

pub(super) fn resolve_tpo_element_color(
    elem_color: data::chart::kline::TpoElementColor,
    palette: &Extended,
    default_color: iced::Color,
) -> iced::Color {
    match elem_color {
        data::chart::kline::TpoElementColor::Auto => default_color,
        data::chart::kline::TpoElementColor::Theme => palette.warning.base.color,
        data::chart::kline::TpoElementColor::Custom([r, g, b]) => iced::Color::from_rgb8(r, g, b),
        other => {
            if let Some([r, g, b]) = other.to_rgb() {
                iced::Color::from_rgb8(r, g, b)
            } else {
                default_color
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_tpo_profiles(
    data_source: &PlotData<KlineDataPoint>,
    tpo_cache: &RefCell<TpoCache>,
    earliest: u64,
    latest: u64,
    effective_period: data::chart::tpo::SessionPeriod,
    clusters: &[data::chart::tpo::SessionCluster],
    tpo_tick: f64,
    is_replay: bool,
) -> Vec<data::chart::tpo::TpoProfile> {
    let mut all_klines: Vec<&Kline> = Vec::new();
    match data_source {
        PlotData::TimeBased(ts) => {
            for dp in ts.datapoints.range(..=latest) {
                all_klines.push(&dp.1.kline);
            }
        }
        PlotData::TickBased(ta) => {
            for dp in &ta.datapoints {
                if dp.kline.time <= latest {
                    all_klines.push(&dp.kline);
                }
            }
        }
    }

    if all_klines.is_empty() {
        return Vec::new();
    }

    let sessions = data::chart::tpo::group_candles_by_period(&all_klines, effective_period);
    if sessions.is_empty() {
        return Vec::new();
    }

    let mut cache = tpo_cache.borrow_mut();
    if (cache.tick_size - tpo_tick).abs() > 1e-9 || cache.period != Some(effective_period) {
        cache.tick_size = tpo_tick;
        cache.period = Some(effective_period);
        cache.sessions.clear();
        if let Ok(anchor) = data::chart::tpo::TpoAnchor::try_from(effective_period) {
            cache.trade_tpo_cache.set_anchor(anchor);
        }
        cache.trade_tpo_cache.set_tick_size(tpo_tick);
    }
    cache.session_ranges.clear();

    let format_session_date = |start: i64| -> String {
        chrono::DateTime::from_timestamp_millis(start)
            .map(|d| {
                use chrono::Timelike;
                let base = d.format("%Y-%m-%d").to_string();
                match effective_period {
                    data::chart::tpo::SessionPeriod::TradingSessions => {
                        let hour = d.hour();
                        let name = if hour < 8 {
                            "Asia"
                        } else if hour < 16 {
                            "London"
                        } else {
                            "NY"
                        };
                        format!("{base} {name}")
                    }
                    data::chart::tpo::SessionPeriod::FourHours => {
                        format!("{base} {:02}:00", d.hour())
                    }
                    _ => base,
                }
            })
            .unwrap_or_else(|| start.to_string())
    };

    // Build or fetch raw profiles from cache
    let mut raw_profiles = Vec::with_capacity(sessions.len());
    for (session_start, session_end, session_candles) in sessions {
        if is_replay && (session_end as u64) > latest {
            continue;
        }
        if session_candles.is_empty() {
            continue;
        }

        let candle_count = session_candles.len();
        let last_c = session_candles[candle_count - 1];
        let last_high = last_c.high.to_f32() as f64;
        let last_low = last_c.low.to_f32() as f64;
        let last_time = last_c.time as i64;

        let profile: Arc<data::chart::tpo::TpoProfile> =
            if let Some(entry) = cache.sessions.get(&session_start) {
                if entry.candle_count == candle_count
                    && (entry.last_candle_high - last_high).abs() < 1e-9
                    && (entry.last_candle_low - last_low).abs() < 1e-9
                    && entry.last_candle_time == last_time
                {
                    Arc::clone(&entry.profile)
                } else {
                    let date_str = format_session_date(session_start);
                    let p = Arc::new(data::chart::tpo::build_tpo_profile(
                        &session_candles,
                        session_start,
                        session_end,
                        &date_str,
                        tpo_tick,
                    ));
                    cache.sessions.insert(
                        session_start,
                        TpoCacheEntry {
                            candle_count,
                            last_candle_high: last_high,
                            last_candle_low: last_low,
                            last_candle_time: last_time,
                            profile: Arc::clone(&p),
                        },
                    );
                    p
                }
            } else {
                let date_str = format_session_date(session_start);
                let p = Arc::new(data::chart::tpo::build_tpo_profile(
                    &session_candles,
                    session_start,
                    session_end,
                    &date_str,
                    tpo_tick,
                ));
                cache.sessions.insert(
                    session_start,
                    TpoCacheEntry {
                        candle_count,
                        last_candle_high: last_high,
                        last_candle_low: last_low,
                        last_candle_time: last_time,
                        profile: Arc::clone(&p),
                    },
                );
                p
            };

        let final_profile =
            if let Some(trade_prof) = cache.trade_tpo_cache.session_profile(session_start) {
                if !trade_prof.matrix.is_empty() {
                    data::chart::tpo::merge_tpo_profiles(&[(*profile).clone(), trade_prof.clone()])
                        .unwrap_or_else(|| (*profile).clone())
                } else {
                    (*profile).clone()
                }
            } else {
                (*profile).clone()
            };

        raw_profiles.push(final_profile);
    }

    // Resolve clusters
    let effective_profiles = if clusters.is_empty() {
        raw_profiles
    } else {
        data::chart::tpo::apply_session_clusters(&raw_profiles, clusters)
    };

    effective_profiles
        .into_iter()
        .filter(|p| {
            let session_start = p.session_start as u64;
            let session_end = p.session_end as u64;
            session_end >= earliest
                && session_start <= latest
                && (!is_replay || session_end <= latest)
                && !p.matrix.is_empty()
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tpo_profiles(
    data_source: &PlotData<KlineDataPoint>,
    tpo_cache: &RefCell<TpoCache>,
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(Price) -> f32,
    interval_to_x: impl Fn(u64) -> f32,
    region: &Rectangle,
    cell_width: f32,
    cell_height: f32,
    scaling: f32,
    exchange_tick_size: f32,
    tpo_tick_size: f64,
    earliest: u64,
    latest: u64,
    visible_min_price: f64,
    visible_max_price: f64,
    show_letters: bool,
    show_ib: bool,
    show_va: bool,
    show_poc: bool,
    show_single_prints: bool,
    effective_period: data::chart::tpo::SessionPeriod,
    clusters: &[data::chart::tpo::SessionCluster],
    split_sessions: &[i64],
    color_scheme: data::chart::kline::TpoColorScheme,
    ib_color: data::chart::kline::TpoElementColor,
    poc_color: data::chart::kline::TpoElementColor,
    single_prints_color: data::chart::kline::TpoElementColor,
    palette: &Extended,
    is_replay: bool,
) {
    let ex_tick = if exchange_tick_size <= 0.0 {
        1.0
    } else {
        exchange_tick_size
    };
    let tpo_tick = if tpo_tick_size <= 0.0 || !tpo_tick_size.is_finite() {
        ex_tick as f64
    } else {
        tpo_tick_size
    };

    // Exact height of one TPO price bin in chart canvas coordinate units
    let tpo_row_height = (tpo_tick as f32 / ex_tick) * cell_height;
    let scaled_row_height = tpo_row_height * scaling;

    let effective_profiles = prepare_tpo_profiles(
        data_source,
        tpo_cache,
        earliest,
        latest,
        effective_period,
        clusters,
        tpo_tick,
        is_replay,
    );

    if effective_profiles.is_empty() {
        return;
    }

    let letter_col_width = (cell_width * 0.75).clamp(1.0, 16.0);
    let min_tick = ((visible_min_price / tpo_tick).floor() as i64).saturating_sub(2);
    let max_tick = ((visible_max_price / tpo_tick).ceil() as i64).saturating_add(2);

    let mut cache = tpo_cache.borrow_mut();
    cache.session_ranges.clear();
    let total_effective = effective_profiles.len();
    for (idx, profile) in effective_profiles.iter().enumerate() {
        let session_start = profile.session_start;
        let session_end = profile.session_end;

        if profile.matrix.is_empty() {
            continue;
        }

        let session_start_x = interval_to_x(session_start as u64);
        let session_end_x = interval_to_x(session_end as u64);
        let session_px_span = (session_end_x - session_start_x).abs().max(cell_width);
        let max_tpo_len = profile.poc.as_ref().map_or(1, |p| p.count.max(1));
        let max_session_draw_width = (session_px_span * 0.95).max(letter_col_width * 2.0);
        let profile_width = ((max_tpo_len as f32) * letter_col_width).min(max_session_draw_width);

        let eff_col_w = (profile_width / (max_tpo_len as f32)).min(letter_col_width);
        let col_gap = (0.8 / scaling).min(eff_col_w * 0.25);
        let draw_w = (eff_col_w - col_gap).max(1.0 / scaling);

        let row_gap = (0.8 / scaling).min(tpo_row_height * 0.25);
        let draw_h = (tpo_row_height - row_gap).max(0.5 / scaling);

        let scaled_eff_col = eff_col_w * scaling;
        let render_text = show_letters && scaled_row_height >= 8.5 && scaled_eff_col >= 7.0;
        let font_size =
            ((scaled_row_height.min(18.0) - 2.0).min(scaled_eff_col - 2.0) / scaling).max(6.0);

        let is_clustered = clusters.iter().any(|c| c.contains(session_start))
            || profile.session_date.ends_with('+');
        let prev_start = if idx > 0 {
            Some(effective_profiles[idx - 1].session_start)
        } else {
            None
        };
        let next_start = if idx + 1 < total_effective {
            Some(effective_profiles[idx + 1].session_start)
        } else {
            None
        };
        let is_session_split = split_sessions.contains(&session_start);

        cache.session_ranges.push(TpoSessionRange {
            session_start,
            session_end,
            prev_start,
            next_start,
            is_clustered,
            is_split_brackets: is_session_split,
        });
        let (va_fill_color, va_line_color, default_poc_color) = match color_scheme {
            data::chart::kline::TpoColorScheme::Theme => (
                palette.primary.base.color.scale_alpha(0.10),
                palette.primary.base.color,
                palette.warning.base.color,
            ),
            data::chart::kline::TpoColorScheme::Custom([r, g, b]) => {
                let base = iced::Color::from_rgb8(r, g, b);
                (
                    base.scale_alpha(0.10),
                    base,
                    iced::Color::from_rgb(0.98, 0.74, 0.18),
                )
            }
            data::chart::kline::TpoColorScheme::Cyan => (
                iced::Color::from_rgba(0.08, 0.80, 0.90, 0.08),
                iced::Color::from_rgb(0.08, 0.80, 0.90),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Emerald => (
                iced::Color::from_rgba(0.15, 0.80, 0.50, 0.08),
                iced::Color::from_rgb(0.15, 0.80, 0.50),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Amber => (
                iced::Color::from_rgba(0.98, 0.70, 0.18, 0.08),
                iced::Color::from_rgb(0.98, 0.70, 0.18),
                iced::Color::from_rgb(0.98, 0.48, 0.22),
            ),
            data::chart::kline::TpoColorScheme::Purple => (
                iced::Color::from_rgba(0.68, 0.36, 0.85, 0.08),
                iced::Color::from_rgb(0.68, 0.36, 0.85),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Red => (
                iced::Color::from_rgba(0.92, 0.30, 0.38, 0.08),
                iced::Color::from_rgb(0.92, 0.30, 0.38),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Blue => (
                iced::Color::from_rgba(0.20, 0.60, 0.95, 0.08),
                iced::Color::from_rgb(0.20, 0.60, 0.95),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Monochrome => (
                iced::Color::from_rgba(0.80, 0.80, 0.80, 0.08),
                iced::Color::from_rgb(0.80, 0.80, 0.80),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
            data::chart::kline::TpoColorScheme::Classic => (
                iced::Color::from_rgba(0.20, 0.65, 0.95, 0.08),
                iced::Color::from_rgb(0.20, 0.65, 0.95),
                iced::Color::from_rgb(0.98, 0.74, 0.18),
            ),
        };
        let poc_color = resolve_tpo_element_color(poc_color, palette, default_poc_color);
        let ib_color =
            resolve_tpo_element_color(ib_color, palette, iced::Color::from_rgb(0.98, 0.74, 0.18));

        // 1. Shaded Value Area (VAH to VAL)
        if show_va && let Some(ref va) = profile.value_area {
            let vah_y = price_to_y(Price::from_f32(va.vah as f32));
            let val_y = price_to_y(Price::from_f32(va.val as f32));
            let top_y = vah_y.min(val_y);
            let bottom_y = vah_y.max(val_y);
            let half_row = tpo_row_height / 2.0;
            let block_top_y = top_y - half_row;
            let block_bottom_y = bottom_y + half_row;
            let h = block_bottom_y - block_top_y;
            let va_width = if is_session_split {
                session_px_span
            } else {
                (profile_width + 16.0).min(max_session_draw_width)
            };
            let line_thickness: f32 = 1.5;

            frame.fill_rectangle(
                Point::new(session_start_x, block_top_y),
                Size::new(va_width, h),
                va_fill_color,
            );

            frame.fill_rectangle(
                Point::new(session_start_x, block_top_y),
                Size::new(va_width, line_thickness),
                va_line_color,
            );
            frame.fill_rectangle(
                Point::new(session_start_x, block_bottom_y - line_thickness),
                Size::new(va_width, line_thickness),
                va_line_color,
            );
        }

        // 2. Single Prints Highlights with Vertical Culling
        if show_single_prints {
            for sp in &profile.single_prints {
                if sp.start_price < visible_min_price || sp.start_price > visible_max_price {
                    continue;
                }
                let y = price_to_y(Price::from_f32(sp.start_price as f32));
                let color = match single_prints_color {
                    data::chart::kline::TpoElementColor::Auto => {
                        if sp.is_tail {
                            iced::Color::from_rgba(0.92, 0.32, 0.45, 0.35)
                        } else {
                            iced::Color::from_rgba(0.68, 0.36, 0.82, 0.25)
                        }
                    }
                    custom_color => {
                        let base = resolve_tpo_element_color(
                            custom_color,
                            palette,
                            iced::Color::from_rgb(0.68, 0.36, 0.82),
                        );
                        if sp.is_tail {
                            base.scale_alpha(0.40)
                        } else {
                            base.scale_alpha(0.28)
                        }
                    }
                };
                frame.fill_rectangle(
                    Point::new(session_start_x, y - tpo_row_height / 2.0),
                    Size::new(eff_col_w + 4.0, tpo_row_height),
                    color,
                );
            }
        }

        // 3. Matrix Blocks / Letters (Split Brackets or Collapsed Profile)
        if is_session_split {
            for (&t, row) in profile.matrix.range(min_tick..=max_tick) {
                if row.is_empty() {
                    continue;
                }

                let bin_price = t as f64 * tpo_tick;
                let y = price_to_y(Price::from_f32(bin_price as f32));
                let cell_y = y - tpo_row_height / 2.0 + row_gap / 2.0;

                let row_indices = profile.bracket_indices.get(&t);

                for (idx_in_row, &bracket) in row.iter().enumerate() {
                    let b_idx = if let Some(indices) = row_indices
                        && let Some(&actual_idx) = indices.get(idx_in_row)
                    {
                        actual_idx as i64
                    } else if bracket.is_ascii_uppercase() {
                        (bracket as u8 - b'A') as i64
                    } else if bracket.is_ascii_lowercase() {
                        26 + (bracket as u8 - b'a') as i64
                    } else {
                        0
                    };

                    let bracket_time = session_start + b_idx * 1_800_000;
                    let col_x = interval_to_x(bracket_time as u64);
                    let next_col_x = interval_to_x((bracket_time + 1_800_000) as u64);
                    let col_span = (next_col_x - col_x).abs().max(eff_col_w);
                    let split_draw_w = (col_span - col_gap).clamp(1.0 / scaling, 24.0);

                    if col_x + split_draw_w < region.x || col_x > region.x + region.width {
                        continue;
                    }

                    let color = bracket_color(bracket, color_scheme, palette);
                    if render_text {
                        frame.fill_rectangle(
                            Point::new(col_x, cell_y),
                            Size::new(split_draw_w, draw_h),
                            color.scale_alpha(0.22),
                        );
                        draw_cluster_text(
                            frame,
                            &bracket.to_string(),
                            Point::new(col_x + split_draw_w / 2.0, y),
                            font_size,
                            color,
                            Alignment::Center,
                            Alignment::Center,
                        );
                    } else {
                        frame.fill_rectangle(
                            Point::new(col_x, cell_y),
                            Size::new(split_draw_w, draw_h),
                            color.scale_alpha(0.88),
                        );
                    }
                }
            }
        } else {
            for (&t, row) in profile.matrix.range(min_tick..=max_tick) {
                if row.is_empty() {
                    continue;
                }

                let bin_price = t as f64 * tpo_tick;
                let y = price_to_y(Price::from_f32(bin_price as f32));

                let min_col = if region.x > session_start_x {
                    ((region.x - session_start_x) / eff_col_w).floor().max(0.0) as usize
                } else {
                    0
                };
                let max_col = (((region.x + region.width - session_start_x) / eff_col_w)
                    .ceil()
                    .max(0.0) as usize)
                    .min(row.len());

                if min_col >= max_col {
                    continue;
                }

                let cell_y = y - tpo_row_height / 2.0 + row_gap / 2.0;

                for (col_offset, &bracket) in row[min_col..max_col].iter().enumerate() {
                    let actual_col = min_col + col_offset;
                    let x = session_start_x + (actual_col as f32) * eff_col_w;
                    let color = bracket_color(bracket, color_scheme, palette);

                    if render_text {
                        frame.fill_rectangle(
                            Point::new(x, cell_y),
                            Size::new(draw_w, draw_h),
                            color.scale_alpha(0.22),
                        );
                        draw_cluster_text(
                            frame,
                            &bracket.to_string(),
                            Point::new(x + draw_w / 2.0, y),
                            font_size,
                            color,
                            Alignment::Center,
                            Alignment::Center,
                        );
                    } else {
                        frame.fill_rectangle(
                            Point::new(x, cell_y),
                            Size::new(draw_w, draw_h),
                            color.scale_alpha(0.88),
                        );
                    }
                }
            }
        }

        // 4. Point of Control (POC) Line
        if show_poc
            && let Some(ref poc) = profile.poc
            && poc.price >= visible_min_price
            && poc.price <= visible_max_price
        {
            let poc_y = price_to_y(Price::from_f32(poc.price as f32));
            let poc_line_width = if is_session_split {
                session_px_span
            } else {
                (profile_width + 24.0).min(max_session_draw_width)
            };
            frame.fill_rectangle(
                Point::new(session_start_x, poc_y - 1.0),
                Size::new(poc_line_width, 2.0),
                poc_color,
            );
        }

        // 5. Initial Balance (IB) and Extensions
        if show_ib
            && let Some(ref ib) = profile.ib
            && ib.high >= visible_min_price
            && ib.low <= visible_max_price
        {
            let ib_h_y = price_to_y(Price::from_f32(ib.high as f32));
            let ib_l_y = price_to_y(Price::from_f32(ib.low as f32));
            let top_y = ib_h_y.min(ib_l_y);
            let h = (ib_h_y - ib_l_y).abs() + tpo_row_height;

            let ib_x_offset = (cell_width * 0.5).clamp(4.0, 10.0);
            let bar_w = (ib_x_offset * 0.6).clamp(2.0, 5.0);
            frame.fill_rectangle(
                Point::new(session_start_x - ib_x_offset, top_y - tpo_row_height / 2.0),
                Size::new(bar_w, h),
                ib_color,
            );

            let ext15_y = price_to_y(Price::from_f32(ib.extension_high_1_5 as f32));
            let ext_w = (profile_width * 0.6).min(max_session_draw_width);
            frame.fill_rectangle(
                Point::new(session_start_x, ext15_y - 0.5),
                Size::new(ext_w, 1.0),
                ib_color.scale_alpha(0.60),
            );

            let ext20_y = price_to_y(Price::from_f32(ib.extension_high_2_0 as f32));
            frame.fill_rectangle(
                Point::new(session_start_x, ext20_y - 0.5),
                Size::new(ext_w, 1.0),
                ib_color.scale_alpha(0.60),
            );
        }
    }
}
