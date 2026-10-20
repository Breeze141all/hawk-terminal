use data::chart::kline::BigTradesConfig;
use exchange::{Trade, util::Price};
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Point, theme::palette::Extended};

/// Renders Big Trades bubbles over visible candle range with O(log N) binary search culling
pub fn draw_big_trades_overlay(
    frame: &mut Frame,
    trades: &[Trade],
    earliest: u64,
    latest: u64,
    interval_to_x: impl Fn(u64) -> f32,
    price_to_y: impl Fn(Price) -> f32,
    palette: &Extended,
    cfg: &BigTradesConfig,
) {
    if trades.is_empty() || earliest > latest {
        return;
    }

    for trade in cfg.filter_visible_trades(trades, earliest, latest) {
        let x = interval_to_x(trade.time);
        let y = price_to_y(trade.price);
        let usd = trade.price.to_f64() * trade.qty as f64;
        let r = cfg.compute_radius(usd);

        let (fill_col, stroke_col) = if trade.is_sell {
            (
                palette.danger.base.color.scale_alpha(cfg.opacity),
                palette.danger.base.color,
            )
        } else {
            (
                palette.success.base.color.scale_alpha(cfg.opacity),
                palette.success.base.color,
            )
        };

        let circle = Path::circle(Point::new(x, y), r);
        frame.fill(&circle, fill_col);
        frame.stroke(
            &circle,
            Stroke::default().with_color(stroke_col).with_width(1.0),
        );
    }
}
