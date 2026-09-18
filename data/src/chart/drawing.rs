use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: uuid::Uuid,
    pub kind: DrawingKind,
    pub color: [f32; 4],
    pub width: f32,
    #[serde(default)]
    pub is_selected: bool,
    #[serde(default)]
    pub is_locked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawingKind {
    /// Horizontal line across entire chart at specific price
    HorizontalLine { price: f32 },
    /// Trendline between point 1 (time_ms, price) and point 2 (time_ms, price)
    Trendline { p1: (u64, f32), p2: (u64, f32) },
    /// Rectangle between corner 1 (time_ms, price) and corner 2 (time_ms, price)
    Rectangle { p1: (u64, f32), p2: (u64, f32) },
    /// Freehand brush stroke composed of sequential points
    Brush { points: Vec<(u64, f32)> },
    /// Polyline path with sequential points ending with an arrowhead
    Path { points: Vec<(u64, f32)> },
    /// Risk/Reward position box (Long or Short)
    Position {
        entry: (u64, f32),
        stop_price: f32,
        target_price: f32,
        is_long: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DrawingTool {
    #[default]
    None,
    Rectangle,
    ShortPosition,
    Path,
    LongPosition,
    Brush,
    Trendline,
    HorizontalLine,
}

impl std::fmt::Display for DrawingTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DrawingTool::None => write!(f, "Cursor"),
            DrawingTool::Rectangle => write!(f, "Rectangle"),
            DrawingTool::ShortPosition => write!(f, "Short Position"),
            DrawingTool::Path => write!(f, "Path"),
            DrawingTool::LongPosition => write!(f, "Long Position"),
            DrawingTool::Brush => write!(f, "Brush"),
            DrawingTool::Trendline => write!(f, "Trendline"),
            DrawingTool::HorizontalLine => write!(f, "Horizontal Line"),
        }
    }
}

impl Drawing {
    pub fn horizontal(price: f32, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::HorizontalLine { price },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    pub fn trendline(p1: (u64, f32), p2: (u64, f32), color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Trendline { p1, p2 },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    pub fn rectangle(p1: (u64, f32), p2: (u64, f32), color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Rectangle { p1, p2 },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    pub fn brush(points: Vec<(u64, f32)>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Brush { points },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    pub fn path(points: Vec<(u64, f32)>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Path { points },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    pub fn position(
        entry: (u64, f32),
        stop_price: f32,
        target_price: f32,
        is_long: bool,
        color: [f32; 4],
        width: f32,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Position {
                entry,
                stop_price,
                target_price,
                is_long,
            },
            color,
            width,
            is_selected: false,
            is_locked: false,
        }
    }

    /// Translates entire drawing in time (milliseconds) and price
    pub fn translate(&mut self, time_delta: i64, price_delta: f32) {
        let shift_time = |t: u64| -> u64 {
            let res = t as i64 + time_delta;
            if res < 0 { 0 } else { res as u64 }
        };
        let shift_price = |p: f32| -> f32 { (p + price_delta).max(0.0000001) };

        match &mut self.kind {
            DrawingKind::HorizontalLine { price } => {
                *price = shift_price(*price);
            }
            DrawingKind::Trendline { p1, p2 } => {
                p1.0 = shift_time(p1.0);
                p1.1 = shift_price(p1.1);
                p2.0 = shift_time(p2.0);
                p2.1 = shift_price(p2.1);
            }
            DrawingKind::Rectangle { p1, p2 } => {
                p1.0 = shift_time(p1.0);
                p1.1 = shift_price(p1.1);
                p2.0 = shift_time(p2.0);
                p2.1 = shift_price(p2.1);
            }
            DrawingKind::Brush { points } | DrawingKind::Path { points } => {
                for pt in points.iter_mut() {
                    pt.0 = shift_time(pt.0);
                    pt.1 = shift_price(pt.1);
                }
            }
            DrawingKind::Position {
                entry,
                stop_price,
                target_price,
                ..
            } => {
                entry.0 = shift_time(entry.0);
                entry.1 = shift_price(entry.1);
                *stop_price = shift_price(*stop_price);
                *target_price = shift_price(*target_price);
            }
        }
    }

    /// Returns control handles for editing
    pub fn handles(&self) -> Vec<(u64, f32)> {
        match &self.kind {
            DrawingKind::HorizontalLine { price } => vec![(0, *price)],
            DrawingKind::Trendline { p1, p2 } => vec![*p1, *p2],
            DrawingKind::Rectangle { p1, p2 } => vec![*p1, (p2.0, p1.1), *p2, (p1.0, p2.1)],
            DrawingKind::Brush { .. } => Vec::new(),
            DrawingKind::Path { points } => points.clone(),
            DrawingKind::Position {
                entry,
                stop_price,
                target_price,
                ..
            } => vec![*entry, (entry.0, *target_price), (entry.0, *stop_price)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drawing_serialization() {
        let d1 = Drawing::horizontal(65000.0, [1.0, 0.5, 0.0, 1.0], 2.0);
        let serialized = serde_json::to_string(&d1).unwrap();
        let d2: Drawing = serde_json::from_str(&serialized).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_drawing_kinds() {
        let t = Drawing::trendline((100, 10.0), (200, 20.0), [0.0, 1.0, 0.0, 1.0], 1.5);
        assert!(matches!(t.kind, DrawingKind::Trendline { .. }));

        let r = Drawing::rectangle((100, 10.0), (200, 20.0), [0.0, 0.0, 1.0, 0.5], 1.0);
        assert!(matches!(r.kind, DrawingKind::Rectangle { .. }));

        let b = Drawing::brush(vec![(10, 1.0), (20, 2.0)], [1.0, 1.0, 1.0, 1.0], 2.0);
        assert!(matches!(b.kind, DrawingKind::Brush { .. }));
        assert!(b.handles().is_empty());

        let pos = Drawing::position(
            (100, 50000.0),
            49000.0,
            53000.0,
            true,
            [0.0, 1.0, 0.0, 1.0],
            1.0,
        );
        assert!(matches!(pos.kind, DrawingKind::Position { .. }));
    }

    #[test]
    fn test_drawing_translation() {
        let mut t = Drawing::trendline((100, 10.0), (200, 20.0), [0.0, 1.0, 0.0, 1.0], 1.5);
        t.translate(50, 5.0);
        if let DrawingKind::Trendline { p1, p2 } = t.kind {
            assert_eq!(p1, (150, 15.0));
            assert_eq!(p2, (250, 25.0));
        } else {
            panic!("Expected Trendline");
        }
    }

    #[test]
    fn test_drawing_lock_serialization() {
        let mut d = Drawing::horizontal(65000.0, [1.0, 0.5, 0.0, 1.0], 2.0);
        assert!(!d.is_locked);
        d.is_locked = true;
        let serialized = serde_json::to_string(&d).unwrap();
        let deserialized: Drawing = serde_json::from_str(&serialized).unwrap();
        assert!(deserialized.is_locked);

        // Test backward compatibility when is_locked is missing from JSON
        let json_without_lock = r#"{"id":"00000000-0000-0000-0000-000000000000","kind":{"HorizontalLine":{"price":65000.0}},"color":[1.0,0.5,0.0,1.0],"width":2.0,"is_selected":false}"#;
        let d_old: Drawing = serde_json::from_str(json_without_lock).unwrap();
        assert!(!d_old.is_locked);
    }
}
