use data::chart::heatmap::GroupedTrade;
use exchange::unit::{Price, PriceStep};

use crate::widget::chart::heatmap::scene::depth_grid::HeatmapPalette;
use crate::widget::chart::heatmap::view::ViewWindow;

pub const CIRCLE_VERTICES: &[[f32; 2]] = &[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
pub const CIRCLE_INDICES: &[u16] = &[0, 1, 2, 2, 3, 0];

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CircleInstance {
    pub y_world: f32,
    pub x_bin_rel: i32,
    pub x_frac: f32,
    pub radius_px: f32,
    _pad: f32,
    pub color: [f32; 4],
}

impl CircleInstance {
    pub const R_MIN_PX: f32 = 1.5;
    const R_MAX_PX: f32 = 25.0;
    const ALPHA: f32 = 0.8;

    pub fn from_trade(
        trade: &GroupedTrade,
        bucket: i64,
        ref_bucket: i64,
        base_price: Price,
        step: PriceStep,
        y_anchor: Option<Price>,
        w: &ViewWindow,
        palette: &HeatmapPalette,
        max_trade_qty: f32,
        trade_size_scale: Option<i32>,
        fallback_radius_px: f32,
    ) -> Self {
        let x_bin_rel = (bucket - ref_bucket).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let x_frac = 0.0;

        let y_world = w.y_center_for_price_texture_aligned(trade.price, base_price, step, y_anchor);

        let q = trade.qty.max(0.0) as f64;
        let max_q = if max_trade_qty <= f32::EPSILON { 1.0 } else { max_trade_qty as f64 };
        let t = (q / max_q).clamp(0.0, 1.0) as f32;
        let radius_px = if let Some(scale_pct) = trade_size_scale {
            let scale_factor = (scale_pct as f32 / 100.0).max(0.0);
            Self::R_MIN_PX + t * (Self::R_MAX_PX - Self::R_MIN_PX) * scale_factor
        } else {
            fallback_radius_px.max(Self::R_MIN_PX)
        };

        let rgba = if trade.is_sell {
            [
                palette.sell_rgb[0],
                palette.sell_rgb[1],
                palette.sell_rgb[2],
                Self::ALPHA,
            ]
        } else {
            [
                palette.buy_rgb[0],
                palette.buy_rgb[1],
                palette.buy_rgb[2],
                Self::ALPHA,
            ]
        };

        Self {
            y_world,
            x_bin_rel,
            x_frac,
            radius_px,
            _pad: 0.0,
            color: rgba,
        }
    }
}
