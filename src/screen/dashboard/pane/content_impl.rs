//! Methods and trait impls of `Content`.
//!
//! Split out of `screen/dashboard/pane.rs` (code unchanged apart from visibility).

use super::*;

impl Content {
    pub(super) fn new_heatmap(
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (enabled_indicators, layout, prev_studies) = if let Content::Heatmap {
            chart,
            indicators,
            studies,
            layout,
        } = current_content
        {
            (
                indicators.clone(),
                chart
                    .as_ref()
                    .map(|c| c.chart_layout())
                    .unwrap_or(layout.clone()),
                chart
                    .as_ref()
                    .map_or(studies.clone(), |c| c.studies.clone()),
            )
        } else {
            (
                vec![HeatmapIndicator::Volume],
                ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
                vec![],
            )
        };

        let basis = settings
            .selected_basis
            .unwrap_or_else(|| Basis::default_heatmap_time(Some(ticker_info)));
        let config = settings.visual_config.clone().and_then(|cfg| cfg.heatmap());

        let chart = HeatmapChart::new(
            layout.clone(),
            basis,
            tick_size,
            &enabled_indicators,
            ticker_info,
            config,
            prev_studies.clone(),
        );

        Content::Heatmap {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            studies: prev_studies,
        }
    }

    pub(super) fn new_kline(
        content_kind: ContentKind,
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
        pane_id: uuid::Uuid,
        layout_id: Option<uuid::Uuid>,
    ) -> Self {
        let (prev_indis, prev_layout, prev_kind_opt, prev_config) = if let Content::Kline {
            chart,
            indicators,
            kind,
            layout,
        } = current_content
        {
            (
                Some(indicators.clone()),
                Some(chart.as_ref().map_or(layout.clone(), |c| c.chart_layout())),
                Some(chart.as_ref().map_or(kind.clone(), |c| c.kind().clone())),
                chart.as_ref().map(|c| c.config()),
            )
        } else {
            (None, None, None, None)
        };

        let prev_was_footprint = prev_kind_opt
            .as_ref()
            .is_some_and(|k| matches!(k, data::chart::KlineChartKind::Footprint { .. }));

        let (default_tf, determined_chart_kind) = match content_kind {
            ContentKind::FootprintChart => (
                Timeframe::M5,
                prev_kind_opt
                    .filter(|k| matches!(k, data::chart::KlineChartKind::Footprint { .. }))
                    .unwrap_or_else(|| data::chart::KlineChartKind::Footprint {
                        clusters: data::chart::kline::ClusterKind::default(),
                        scaling: data::chart::kline::ClusterScaling::default(),
                        studies: vec![],
                        show_bottom_volume: false,
                        aggregate: false,
                        aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                        history_lookback_days: None,
                    }),
            ),
            ContentKind::CandlestickChart => (Timeframe::M15, data::chart::KlineChartKind::Candles),
            ContentKind::TpoChart => (
                Timeframe::M30,
                prev_kind_opt
                    .filter(|k| matches!(k, data::chart::KlineChartKind::Tpo { .. }))
                    .unwrap_or(data::chart::KlineChartKind::Tpo {
                        show_candles: true,
                        show_letters: true,
                        show_ib: true,
                        show_va: true,
                        show_poc: true,
                        show_single_prints: true,
                        tick_step: Default::default(),
                        period: Default::default(),
                        clusters: Vec::new(),
                        split_sessions: Vec::new(),
                        color_scheme: Default::default(),
                        ib_color: Default::default(),
                        poc_color: Default::default(),
                        single_prints_color: Default::default(),
                    }),
            ),
            _ => unreachable!("invalid content kind for kline chart"),
        };

        let basis = settings.selected_basis.unwrap_or(Basis::Time(default_tf));

        let enabled_indicators = {
            let available = KlineIndicator::for_market(ticker_info.market_type());
            let is_footprint = matches!(content_kind, ContentKind::FootprintChart);

            if is_footprint && !prev_was_footprint {
                vec![]
            } else {
                prev_indis.map_or_else(Vec::new, |indis| {
                    indis
                        .into_iter()
                        .filter(|i| available.contains(i))
                        .collect()
                })
            }
        };

        let splits = if enabled_indicators.is_empty() {
            vec![]
        } else {
            let main_chart_split: f32 = 0.8;
            let mut splits_vec = vec![main_chart_split];
            let num_indicators = enabled_indicators.len();

            let indicator_total_height_ratio = 1.0 - main_chart_split;
            let height_per_indicator_pane = indicator_total_height_ratio / num_indicators as f32;

            let mut current_split_pos = main_chart_split;
            for _ in 0..(num_indicators - 1) {
                current_split_pos += height_per_indicator_pane;
                splits_vec.push(current_split_pos);
            }
            splits_vec
        };

        let layout = prev_layout
            .filter(|l| l.splits.len() == splits.len())
            .unwrap_or(ViewConfig {
                splits,
                autoscale: Some(data::chart::Autoscale::FitToVisible),
            });

        let kline_config = settings
            .visual_config
            .as_ref()
            .and_then(|cfg| cfg.kline())
            .or(prev_config);

        let mut chart = KlineChart::new(
            layout.clone(),
            basis,
            tick_size,
            &[],
            vec![],
            &enabled_indicators,
            ticker_info,
            &determined_chart_kind,
            kline_config,
        );
        let symbol = ticker_info.ticker.display_symbol_and_type().0;
        chart.alerts = data::AlertStore::for_symbol_and_pane(&symbol, pane_id);
        chart.drawings = if let Some(lid) = layout_id {
            data::DrawingStore::for_symbol_layout_and_pane(&symbol, lid, pane_id)
        } else {
            data::DrawingStore::for_symbol_and_pane(&symbol, pane_id)
        };

        Content::Kline {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            kind: determined_chart_kind,
        }
    }

    pub(super) fn placeholder(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Starter => Content::Starter,
            ContentKind::CandlestickChart => Content::Kline {
                chart: None,
                indicators: vec![],
                kind: data::chart::KlineChartKind::Candles,
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::TpoChart => Content::Kline {
                chart: None,
                indicators: vec![],
                kind: data::chart::KlineChartKind::Tpo {
                    show_candles: true,
                    show_letters: true,
                    show_ib: true,
                    show_va: true,
                    show_poc: true,
                    show_single_prints: true,
                    tick_step: Default::default(),
                    period: Default::default(),
                    clusters: Vec::new(),
                    split_sessions: Vec::new(),
                    color_scheme: Default::default(),
                    ib_color: Default::default(),
                    poc_color: Default::default(),
                    single_prints_color: Default::default(),
                },
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::FootprintChart => Content::Kline {
                chart: None,
                indicators: vec![],
                kind: data::chart::KlineChartKind::Footprint {
                    clusters: data::chart::kline::ClusterKind::default(),
                    scaling: data::chart::kline::ClusterScaling::default(),
                    studies: vec![],
                    show_bottom_volume: false,
                    aggregate: false,
                    aggregate_exchanges: data::chart::kline::default_aggregate_exchanges(),
                    history_lookback_days: None,
                },
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::HeatmapChart => Content::Heatmap {
                chart: None,
                indicators: vec![HeatmapIndicator::Volume],
                studies: vec![],
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
            },
            #[cfg(not(feature = "shader-heatmap"))]
            ContentKind::ShaderHeatmap => Self::placeholder(ContentKind::HeatmapChart),
            #[cfg(feature = "shader-heatmap")]
            ContentKind::ShaderHeatmap => Content::ShaderHeatmap {
                chart: None,
                indicators: vec![HeatmapIndicator::Volume],
                studies: vec![HeatmapStudy::VolumeProfile(
                    data::chart::heatmap::ProfileKind::default(),
                )],
            },
            ContentKind::ComparisonChart => Content::Comparison(None),
            ContentKind::TimeAndSales => Content::TimeAndSales(None),
            ContentKind::Ladder => Content::Ladder(None),
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        match self {
            Content::Heatmap { chart, .. } => Some(chart.as_ref()?.last_update()),
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { chart, .. } => Some(chart.as_ref()?.last_tick?),
            Content::Kline { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::TimeAndSales(panel) => Some(panel.as_ref()?.last_update()),
            Content::Ladder(panel) => Some(panel.as_ref()?.last_update()),
            Content::Comparison(chart) => Some(chart.as_ref()?.last_update()),
            Content::Starter => None,
        }
    }

    pub fn chart_kind(&self) -> Option<data::chart::KlineChartKind> {
        match self {
            Content::Kline { chart, .. } => Some(chart.as_ref()?.kind().clone()),
            _ => None,
        }
    }

    pub fn toggle_indicator(&mut self, indicator: UiIndicator) {
        match (self, indicator) {
            (
                Content::Heatmap {
                    chart, indicators, ..
                },
                UiIndicator::Heatmap(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            #[cfg(feature = "shader-heatmap")]
            (
                Content::ShaderHeatmap {
                    chart, indicators, ..
                },
                UiIndicator::Heatmap(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            (
                Content::Kline {
                    chart, indicators, ..
                },
                UiIndicator::Kline(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            _ => {
                log::error!(
                    "Cannot toggle indicator {indicator:?} on non-indicator pane (ignored)"
                );
            }
        }
    }

    pub fn reorder_indicators(&mut self, event: &column_drag::DragEvent) {
        match self {
            Content::Heatmap { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::Kline { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => {
                log::error!("Cannot reorder indicators on {self} pane (ignored)");
            }
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { .. } => {
                log::error!("Cannot reorder indicators on {self} pane (ignored)");
            }
        }
    }

    pub fn change_visual_config(&mut self, config: VisualConfig) {
        match (self, config) {
            (Content::Kline { chart: Some(c), .. }, VisualConfig::Kline(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::Heatmap { chart: Some(c), .. }, VisualConfig::Heatmap(cfg)) => {
                c.set_visual_config(cfg);
            }
            #[cfg(feature = "shader-heatmap")]
            (Content::ShaderHeatmap { chart: Some(c), .. }, VisualConfig::Heatmap(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::TimeAndSales(Some(panel)), VisualConfig::TimeAndSales(cfg)) => {
                panel.config = cfg;
            }
            (Content::Ladder(Some(panel)), VisualConfig::Ladder(cfg)) => {
                panel.config = cfg;
            }
            (Content::Comparison(Some(chart)), VisualConfig::Comparison(cfg)) => {
                chart.config = cfg;
            }
            _ => {}
        }
    }

    pub fn studies(&self) -> Option<data::chart::Study> {
        match &self {
            Content::Heatmap { studies, .. } => Some(data::chart::Study::Heatmap(studies.clone())),
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { studies, .. } => {
                Some(data::chart::Study::Heatmap(studies.clone()))
            }
            Content::Kline { kind, .. } => {
                if let data::chart::KlineChartKind::Footprint { studies, .. } = kind {
                    Some(data::chart::Study::Footprint(studies.clone()))
                } else {
                    None
                }
            }
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => None,
        }
    }

    pub fn clusters(&self) -> Option<data::chart::kline::ClusterKind> {
        match self {
            Content::Kline {
                kind: data::chart::KlineChartKind::Footprint { clusters, .. },
                ..
            } => Some(*clusters),
            _ => None,
        }
    }

    pub fn update_studies(&mut self, studies: data::chart::Study) {
        match (self, studies) {
            (
                Content::Heatmap {
                    chart,
                    studies: previous,
                    ..
                },
                data::chart::Study::Heatmap(studies),
            ) => {
                chart
                    .as_mut()
                    .expect("heatmap chart not initialized")
                    .studies = studies.clone();
                *previous = studies;
            }
            #[cfg(feature = "shader-heatmap")]
            (
                Content::ShaderHeatmap {
                    chart,
                    studies: previous,
                    ..
                },
                data::chart::Study::Heatmap(studies),
            ) => {
                chart
                    .as_mut()
                    .expect("shader heatmap chart not initialized")
                    .studies = studies.clone();
                *previous = studies;
            }
            (Content::Kline { chart, kind, .. }, data::chart::Study::Footprint(studies)) => {
                chart
                    .as_mut()
                    .expect("kline chart not initialized")
                    .set_studies(studies.clone());
                if let data::chart::KlineChartKind::Footprint {
                    studies: k_studies, ..
                } = kind
                {
                    *k_studies = studies;
                }
            }
            _ => {}
        }
    }

    pub fn kind(&self) -> ContentKind {
        match self {
            Content::Heatmap { .. } => ContentKind::HeatmapChart,
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { .. } => ContentKind::ShaderHeatmap,
            Content::Kline { kind, .. } => match kind {
                data::chart::KlineChartKind::Footprint { .. } => ContentKind::FootprintChart,
                data::chart::KlineChartKind::Candles => ContentKind::CandlestickChart,
                data::chart::KlineChartKind::Tpo { .. } => ContentKind::TpoChart,
            },
            Content::TimeAndSales(_) => ContentKind::TimeAndSales,
            Content::Ladder(_) => ContentKind::Ladder,
            Content::Comparison(_) => ContentKind::ComparisonChart,
            Content::Starter => ContentKind::Starter,
        }
    }

    pub fn update_theme(&mut self, theme: &iced_core::Theme) {
        // Only the WGPU heatmap caches theme colors; the other charts read them per frame.
        #[cfg(not(feature = "shader-heatmap"))]
        let _ = theme;
        #[cfg(feature = "shader-heatmap")]
        if let Content::ShaderHeatmap { chart: Some(c), .. } = self {
            c.update_theme(theme);
        }
    }

    pub(super) fn initialized(&self) -> bool {
        match self {
            Content::Heatmap { chart, .. } => chart.is_some(),
            #[cfg(feature = "shader-heatmap")]
            Content::ShaderHeatmap { chart, .. } => chart.is_some(),
            Content::Kline { chart, .. } => chart.is_some(),
            Content::TimeAndSales(panel) => panel.is_some(),
            Content::Ladder(panel) => panel.is_some(),
            Content::Comparison(chart) => chart.is_some(),
            Content::Starter => true,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind())
    }
}

impl Content {
    /// Whether a visual config of `cfg`'s kind applies to this pane's chart family.
    pub fn accepts_visual_config(&self, cfg: &VisualConfig) -> bool {
        match (cfg, self) {
            (VisualConfig::Kline(_), Content::Kline { .. })
            | (VisualConfig::Heatmap(_), Content::Heatmap { .. })
            | (VisualConfig::TimeAndSales(_), Content::TimeAndSales(_))
            | (VisualConfig::Comparison(_), Content::Comparison(_)) => true,
            #[cfg(feature = "shader-heatmap")]
            (VisualConfig::Heatmap(_), Content::ShaderHeatmap { .. }) => true,
            _ => false,
        }
    }
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(feature = "shader-heatmap")]
        if matches!(
            (self, other),
            (Content::ShaderHeatmap { .. }, Content::ShaderHeatmap { .. })
        ) {
            return true;
        }
        matches!(
            (self, other),
            (Content::Starter, Content::Starter)
                | (Content::Heatmap { .. }, Content::Heatmap { .. })
                | (Content::Kline { .. }, Content::Kline { .. })
                | (Content::TimeAndSales(_), Content::TimeAndSales(_))
                | (Content::Ladder(_), Content::Ladder(_))
        )
    }
}
