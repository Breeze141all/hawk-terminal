//! The cached main layer of the kline canvas: candles or footprints, studies, overlays
//! and the last price line. Split out of `canvas::Program::draw` (ROADMAP #131); the body
//! is the former closure, unchanged.

use super::*;

impl KlineChart {
    pub(super) fn draw_main_layer(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        palette: &Extended,
        bounds: Rectangle,
    ) {
        let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);

        frame.translate(center);
        frame.scale(chart.scaling);
        frame.translate(chart.translation);

        let region = chart.visible_region(frame.size());
        let (earliest, raw_latest) = chart.interval_range(&region);
        let latest = if let Some(ref rep) = self.replay {
            if rep.active && rep.cutoff_time > 0 {
                raw_latest.min(rep.cutoff_time)
            } else {
                raw_latest
            }
        } else {
            raw_latest
        };

        if earliest > latest {
            chart.draw_last_price_line(frame, palette, region);
            return;
        }

        let (p_high, p_low) = chart.price_range(&region);
        let visible_max_price = p_high.to_f32_lossy().max(p_low.to_f32_lossy()) as f64;
        let visible_min_price = p_high.to_f32_lossy().min(p_low.to_f32_lossy()) as f64;

        let price_to_y = |price| chart.price_to_y(price);
        let interval_to_x = |interval| chart.interval_to_x(interval);

        if self.indicators[KlineIndicator::LiquidationHeatmap].is_some() {
            let current_price = chart
                .last_price
                .as_ref()
                .map(|lp| lp.price().to_f32())
                .unwrap_or_else(|| chart.base_price_y.to_f32_lossy());
            draw_liquidation_heatmap(
                &self.liquidation_heatmap,
                frame,
                &region,
                price_to_y,
                interval_to_x,
                earliest,
                latest,
                current_price,
                self.config.liq_show_bands,
                self.config.liq_show_histogram,
                palette.is_dark,
            );
        }

        match &self.kind {
            KlineChartKind::Footprint {
                clusters,
                scaling,
                studies,
                show_bottom_volume,
                ..
            } => {
                let (highest, lowest) = chart.price_range(&region);

                let max_cluster_qty = self.calc_qty_scales(
                    earliest,
                    latest,
                    highest,
                    lowest,
                    chart.tick_size,
                    *clusters,
                );

                let cell_height_unscaled = chart.cell_height * chart.scaling;
                let cell_width_unscaled = chart.cell_width * chart.scaling;

                let text_screen_size = {
                    let text_size_from_height = (cell_height_unscaled * 0.72).clamp(8.0, 13.0);
                    let text_size_from_width = (cell_width_unscaled * 0.12).clamp(8.0, 13.0);

                    text_size_from_height.min(text_size_from_width)
                };
                let text_size = text_screen_size / chart.scaling;

                let candle_width = 0.1 * chart.cell_width;
                let content_spacing = ContentGaps::from_view(candle_width, chart.scaling);

                let imbalance = studies.iter().find_map(|study| {
                    if let FootprintStudy::Imbalance {
                        threshold,
                        color_scale,
                        ignore_zeros,
                    } = study
                    {
                        Some((*threshold, *color_scale, *ignore_zeros))
                    } else {
                        None
                    }
                });

                let min_w = clusters.min_footprint_width();
                let lod = lod_level(cell_width_unscaled, cell_height_unscaled, min_w);
                let show_text = lod == LodLevel::Full;
                let has_npoc = studies
                    .iter()
                    .any(|s| matches!(s, FootprintStudy::NPoC { .. }));

                if lod != LodLevel::Candle {
                    draw_all_npocs(
                        &self.data_source,
                        frame,
                        price_to_y,
                        interval_to_x,
                        candle_width,
                        chart.cell_width,
                        chart.cell_height,
                        palette,
                        studies,
                        earliest,
                        latest,
                        *clusters,
                        content_spacing,
                        imbalance.is_some(),
                    );

                    render_data_source(
                        &self.data_source,
                        frame,
                        earliest,
                        latest,
                        interval_to_x,
                        |frame, x_position, kline, trades| {
                            let is_replay = self.replay.is_some();
                            let sim_trades;
                            let effective_trades = if is_replay
                                && trades.is_empty()
                                && (kline.volume_f32().0 + kline.volume_f32().1) > 0.0
                            {
                                sim_trades = self.get_or_simulate_footprint(kline, chart.tick_size);
                                &sim_trades
                            } else {
                                trades
                            };

                            let cluster_scaling = effective_cluster_qty(
                                *scaling,
                                max_cluster_qty,
                                effective_trades,
                                *clusters,
                            );

                            draw_clusters(
                                frame,
                                price_to_y,
                                x_position,
                                chart.cell_width,
                                chart.cell_height,
                                chart.scaling,
                                candle_width,
                                cluster_scaling,
                                palette,
                                text_size,
                                self.tick_size(),
                                show_text,
                                imbalance,
                                kline,
                                effective_trades,
                                *clusters,
                                content_spacing,
                                has_npoc,
                                self.config.show_footprint_summary,
                            );

                            // ClusterSearch highlight overlay (supports multiple search rules)
                            for study in studies {
                                if let FootprintStudy::ClusterSearch {
                                    min_volume,
                                    min_delta,
                                    side,
                                    style,
                                    color,
                                    ..
                                } = study
                                {
                                    draw_cluster_search_highlights(
                                        frame,
                                        price_to_y,
                                        x_position,
                                        chart.cell_width,
                                        chart.cell_height,
                                        chart.scaling,
                                        effective_trades,
                                        *min_volume,
                                        *min_delta,
                                        *side,
                                        *style,
                                        *color,
                                    );
                                }
                            }
                        },
                    );
                } else {
                    // LOD::Candle — render plain thin candles, with optional classic bottom volume
                    let plain_candle_width = (chart.cell_width * 0.8).max(1.0);
                    let show_vol = *show_bottom_volume;
                    let max_vol = if show_vol && earliest <= latest {
                        match &self.data_source {
                            PlotData::TimeBased(ts) => ts
                                .datapoints
                                .range(earliest..=latest)
                                .map(|(_, dp)| dp.kline.volume_f32().0 + dp.kline.volume_f32().1)
                                .fold(0.0_f32, f32::max),
                            PlotData::TickBased(ta) => {
                                let e = earliest as usize;
                                let l = latest as usize;
                                ta.datapoints
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| *i >= e && *i <= l)
                                    .map(|(_, dp)| {
                                        dp.kline.volume_f32().0 + dp.kline.volume_f32().1
                                    })
                                    .fold(0.0_f32, f32::max)
                            }
                        }
                    } else {
                        0.0
                    };
                    let max_vol_h = region.height * 0.18;
                    let bottom_y = region.y + region.height;

                    render_data_source(
                        &self.data_source,
                        frame,
                        earliest,
                        latest,
                        interval_to_x,
                        |frame, x_position, kline, trades| {
                            draw_candle_dp(
                                frame,
                                price_to_y,
                                plain_candle_width,
                                palette,
                                x_position,
                                kline,
                            );

                            if show_vol && max_vol > 0.0 {
                                let vol = kline.volume_f32().0 + kline.volume_f32().1;
                                let h = (vol / max_vol) * max_vol_h;
                                let vol_color = if kline.close >= kline.open {
                                    palette.success.base.color.scale_alpha(0.50)
                                } else {
                                    palette.danger.base.color.scale_alpha(0.50)
                                };
                                frame.fill_rectangle(
                                    Point::new(x_position - plain_candle_width / 2.0, bottom_y - h),
                                    Size::new(plain_candle_width, h),
                                    vol_color,
                                );
                            }

                            for study in studies {
                                if let FootprintStudy::ClusterSearch {
                                    min_volume,
                                    min_delta,
                                    side,
                                    style,
                                    color,
                                    ..
                                } = study
                                {
                                    draw_cluster_search_highlights(
                                        frame,
                                        price_to_y,
                                        x_position,
                                        chart.cell_width,
                                        chart.cell_height,
                                        chart.scaling,
                                        trades,
                                        *min_volume,
                                        *min_delta,
                                        *side,
                                        *style,
                                        *color,
                                    );
                                }
                            }
                        },
                    );
                }

                for study in studies {
                    if let FootprintStudy::BigTrades {
                        min_usd,
                        min_radius,
                        max_radius,
                        scale,
                        opacity,
                    } = study
                    {
                        let big_cfg = data::chart::kline::BigTradesConfig {
                            min_usd: *min_usd,
                            min_radius: *min_radius,
                            max_radius: *max_radius,
                            scale: *scale,
                            opacity: *opacity,
                        };
                        crate::widget::chart::big_trades::draw_big_trades_overlay(
                            frame,
                            &self.raw_trades,
                            earliest,
                            latest,
                            interval_to_x,
                            price_to_y,
                            palette,
                            &big_cfg,
                        );
                    }
                }
            }
            KlineChartKind::Candles => {
                let candle_width = chart.cell_width * 0.8;

                render_data_source(
                    &self.data_source,
                    frame,
                    earliest,
                    latest,
                    interval_to_x,
                    |frame, x_position, kline, _| {
                        draw_candle_dp(frame, price_to_y, candle_width, palette, x_position, kline);
                    },
                );
            }
            KlineChartKind::Tpo {
                show_candles,
                show_letters,
                show_ib,
                show_va,
                show_poc,
                show_single_prints,
                tick_step,
                period: _,
                clusters,
                split_sessions,
                color_scheme,
                ib_color,
                poc_color,
                single_prints_color,
            } => {
                let base_price = chart.base_price_y.to_f32_lossy() as f64;
                let exchange_tick = chart.tick_size.to_f32_lossy() as f64;
                let tpo_tick = data::chart::tpo::calculate_tpo_tick_size(
                    base_price,
                    exchange_tick,
                    tick_step.multiplier(),
                );
                let effective_period = self.kind.effective_tpo_period();

                draw_tpo_profiles(
                    &self.data_source,
                    &self.tpo_cache,
                    frame,
                    price_to_y,
                    interval_to_x,
                    &region,
                    chart.cell_width,
                    chart.cell_height,
                    chart.scaling,
                    chart.tick_size.to_f32_lossy(),
                    tpo_tick,
                    earliest,
                    latest,
                    visible_min_price,
                    visible_max_price,
                    *show_letters,
                    *show_ib,
                    *show_va,
                    *show_poc,
                    *show_single_prints,
                    effective_period,
                    clusters,
                    split_sessions,
                    *color_scheme,
                    *ib_color,
                    *poc_color,
                    *single_prints_color,
                    palette,
                    self.is_replay_active(),
                );

                if *show_candles {
                    let candle_width = chart.cell_width * 0.8;
                    render_data_source(
                        &self.data_source,
                        frame,
                        earliest,
                        latest,
                        interval_to_x,
                        |frame, x_position, kline, _| {
                            draw_candle_dp(
                                frame,
                                price_to_y,
                                candle_width,
                                palette,
                                x_position,
                                kline,
                            );
                        },
                    );
                }
            }
        }

        if self.indicators[KlineIndicator::Vwap].is_some() {
            draw_vwap_overlay(
                &self.data_source,
                frame,
                price_to_y,
                interval_to_x,
                earliest,
                latest,
            );
        }

        if self.indicators[KlineIndicator::RollingVwap].is_some()
            && (self.config.rolling_vwap_show_7d
                || self.config.rolling_vwap_show_30d
                || self.config.rolling_vwap_show_90d
                || self.config.rolling_vwap_show_365d)
        {
            let rolling_indicator = self.indicators[KlineIndicator::RollingVwap]
                    .as_ref()
                    .and_then(|box_indi| box_indi.as_any())
                    .and_then(|any| any.downcast_ref::<crate::chart::indicator::kline::rolling_vwap::RollingVwapIndicator>());

            draw_rolling_vwap_overlay(
                rolling_indicator,
                &self.data_source,
                frame,
                price_to_y,
                interval_to_x,
                earliest,
                latest,
                &self.config,
            );
        }

        if self.indicators[KlineIndicator::Tpo].is_some()
            && !matches!(self.kind, KlineChartKind::Tpo { .. })
        {
            let base_price = chart.base_price_y.to_f32_lossy() as f64;
            let exchange_tick = chart.tick_size.to_f32_lossy() as f64;
            let tpo_tick = data::chart::tpo::calculate_tpo_tick_size(base_price, exchange_tick, 0);
            draw_tpo_profiles(
                &self.data_source,
                &self.tpo_cache,
                frame,
                price_to_y,
                interval_to_x,
                &region,
                chart.cell_width,
                chart.cell_height,
                chart.scaling,
                chart.tick_size.to_f32_lossy(),
                tpo_tick,
                earliest,
                latest,
                visible_min_price,
                visible_max_price,
                true,
                false,
                true,
                true,
                true,
                data::chart::tpo::SessionPeriod::Daily,
                &[],
                &[],
                data::chart::kline::TpoColorScheme::default(),
                data::chart::kline::TpoElementColor::default(),
                data::chart::kline::TpoElementColor::default(),
                data::chart::kline::TpoElementColor::default(),
                palette,
                self.is_replay_active(),
            );
        }

        // Pass 3.5: Render scripted indicator overlays
        crate::chart::indicator::kline::scripted::draw_scripted_indicator_overlays(
            &self.indicator_instances,
            &self.indicator_host,
            &self.data_source,
            frame,
            price_to_y,
            interval_to_x,
            earliest,
            latest,
            palette,
            &region,
        );

        chart.draw_last_price_line(frame, palette, region);
    }
}
