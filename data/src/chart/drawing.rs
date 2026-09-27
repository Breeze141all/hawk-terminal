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
    #[serde(default)]
    pub is_synced: bool,
    #[serde(default)]
    pub pane_id: Option<uuid::Uuid>,
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
        #[serde(default)]
        end_time: Option<u64>,
        #[serde(default)]
        style: Option<PositionStyle>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PositionStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profit_color: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_color: Option<[f32; 4]>,
    #[serde(default = "default_entry_color")]
    pub entry_color: [f32; 4],
    #[serde(default = "default_true")]
    pub show_price_path: bool,
}

pub fn default_profit_color() -> [f32; 4] {
    [0.12, 0.78, 0.42, 0.1]
}

pub fn default_stop_color() -> [f32; 4] {
    [0.88, 0.24, 0.24, 0.1]
}

pub fn default_entry_color() -> [f32; 4] {
    [0.85, 0.9, 0.95, 0.9]
}

pub fn default_true() -> bool {
    true
}

impl Default for PositionStyle {
    fn default() -> Self {
        Self {
            profit_color: None,
            stop_color: None,
            entry_color: default_entry_color(),
            show_price_path: true,
        }
    }
}

impl PositionStyle {
    pub fn resolved_profit_color(&self, fallback: [f32; 4]) -> [f32; 4] {
        self.profit_color.unwrap_or(fallback)
    }

    pub fn resolved_stop_color(&self, fallback: [f32; 4]) -> [f32; 4] {
        self.stop_color.unwrap_or(fallback)
    }
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
            is_synced: false,
            pane_id: None,
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
            is_synced: false,
            pane_id: None,
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
            is_synced: false,
            pane_id: None,
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
            is_synced: false,
            pane_id: None,
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
            is_synced: false,
            pane_id: None,
        }
    }

    pub fn position(
        entry: (u64, f32),
        stop_price: f32,
        target_price: f32,
        is_long: bool,
        end_time: Option<u64>,
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
                end_time,
                style: Some(PositionStyle::default()),
            },
            color,
            width,
            is_selected: false,
            is_locked: false,
            is_synced: false,
            pane_id: None,
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
                end_time,
                ..
            } => {
                entry.0 = shift_time(entry.0);
                entry.1 = shift_price(entry.1);
                *stop_price = shift_price(*stop_price);
                *target_price = shift_price(*target_price);
                if let Some(et) = end_time {
                    *et = shift_time(*et);
                }
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
                end_time,
                ..
            } => {
                let et = end_time.unwrap_or(entry.0);
                vec![
                    *entry,
                    (entry.0, *target_price),
                    (entry.0, *stop_price),
                    (et, entry.1),
                ]
            }
        }
    }

    pub fn position_style(&self) -> PositionStyle {
        match &self.kind {
            DrawingKind::Position { style: Some(s), .. } => *s,
            _ => PositionStyle::default(),
        }
    }

    pub fn set_position_profit_color(&mut self, color: Option<[f32; 4]>) {
        if let DrawingKind::Position { ref mut style, .. } = self.kind {
            let mut s = style.unwrap_or_default();
            s.profit_color = color;
            *style = Some(s);
        }
    }

    pub fn set_position_stop_color(&mut self, color: Option<[f32; 4]>) {
        if let DrawingKind::Position { ref mut style, .. } = self.kind {
            let mut s = style.unwrap_or_default();
            s.stop_color = color;
            *style = Some(s);
        }
    }

    pub fn set_position_entry_color(&mut self, color: [f32; 4]) {
        if let DrawingKind::Position { ref mut style, .. } = self.kind {
            let mut s = style.unwrap_or_default();
            s.entry_color = color;
            *style = Some(s);
        }
    }

    pub fn set_position_show_price_path(&mut self, show: bool) {
        if let DrawingKind::Position { ref mut style, .. } = self.kind {
            let mut s = style.unwrap_or_default();
            s.show_price_path = show;
            *style = Some(s);
        }
    }
}

use std::sync::{LazyLock, RwLock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingRecord {
    pub ticker_symbol: String,
    pub drawing: Drawing,
}

static GLOBAL_DRAWINGS: LazyLock<RwLock<Vec<DrawingRecord>>> =
    LazyLock::new(|| RwLock::new(crate::load_drawings()));

pub struct DrawingStore;

impl DrawingStore {
    /// Retrieve a clone of all drawing records
    pub fn all() -> Vec<DrawingRecord> {
        GLOBAL_DRAWINGS.read().unwrap().clone()
    }

    /// Retrieve all drawings for a specific ticker symbol
    pub fn for_symbol(symbol: &str) -> Vec<Drawing> {
        GLOBAL_DRAWINGS
            .read()
            .unwrap()
            .iter()
            .filter(|r| r.ticker_symbol == symbol)
            .map(|r| r.drawing.clone())
            .collect()
    }

    /// Retrieve drawings for a specific ticker symbol visible to a given pane.
    /// Returns drawings that are synced to all charts, drawings specific to this pane,
    /// or legacy drawings that have no pane_id assigned yet.
    pub fn for_symbol_and_pane(symbol: &str, pane_id: uuid::Uuid) -> Vec<Drawing> {
        GLOBAL_DRAWINGS
            .read()
            .unwrap()
            .iter()
            .filter(|r| {
                r.ticker_symbol == symbol
                    && (r.drawing.is_synced
                        || r.drawing.pane_id == Some(pane_id)
                        || r.drawing.pane_id.is_none())
            })
            .map(|r| r.drawing.clone())
            .collect()
    }

    /// Add or update a drawing for a symbol and persist to disk
    pub fn add(symbol: impl Into<String>, drawing: Drawing) {
        let mut drawings = GLOBAL_DRAWINGS.write().unwrap();
        if let Some(existing) = drawings.iter_mut().find(|r| r.drawing.id == drawing.id) {
            existing.drawing = drawing;
        } else {
            drawings.push(DrawingRecord {
                ticker_symbol: symbol.into(),
                drawing,
            });
        }
        let _ = crate::save_drawings(&drawings);
    }

    /// Update an existing drawing for a symbol and persist to disk
    pub fn update(symbol: &str, drawing: Drawing) {
        let mut drawings = GLOBAL_DRAWINGS.write().unwrap();
        if let Some(record) = drawings.iter_mut().find(|r| r.drawing.id == drawing.id) {
            record.drawing = drawing;
            record.ticker_symbol = symbol.to_string();
        } else {
            drawings.push(DrawingRecord {
                ticker_symbol: symbol.to_string(),
                drawing,
            });
        }
        let _ = crate::save_drawings(&drawings);
    }

    /// Remove a drawing by ID and persist to disk
    pub fn remove(id: uuid::Uuid) {
        let mut drawings = GLOBAL_DRAWINGS.write().unwrap();
        drawings.retain(|r| r.drawing.id != id);
        let _ = crate::save_drawings(&drawings);
    }

    /// Clear unlocked drawings for a specific symbol and persist to disk
    pub fn clear_for_symbol(symbol: &str) {
        let mut drawings = GLOBAL_DRAWINGS.write().unwrap();
        drawings.retain(|r| r.ticker_symbol != symbol || r.drawing.is_locked);
        let _ = crate::save_drawings(&drawings);
    }

    /// Clear unlocked drawings for a specific symbol visible to a given pane.
    /// Drawings belonging exclusively to another pane remain untouched.
    pub fn clear_for_symbol_and_pane(symbol: &str, pane_id: uuid::Uuid) {
        let mut drawings = GLOBAL_DRAWINGS.write().unwrap();
        drawings.retain(|r| {
            if r.ticker_symbol != symbol || r.drawing.is_locked {
                return true;
            }
            if !r.drawing.is_synced
                && r.drawing.pane_id.is_some()
                && r.drawing.pane_id != Some(pane_id)
            {
                return true;
            }
            false
        });
        let _ = crate::save_drawings(&drawings);
    }

    /// Force save all drawings to disk
    pub fn save() {
        let drawings = GLOBAL_DRAWINGS.read().unwrap();
        let _ = crate::save_drawings(&drawings);
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
            Some(200),
            [0.0, 1.0, 0.0, 1.0],
            1.0,
        );
        assert!(matches!(pos.kind, DrawingKind::Position { .. }));
        assert_eq!(pos.handles().len(), 4);
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

        let mut p = Drawing::position(
            (100, 50000.0),
            49000.0,
            53000.0,
            true,
            Some(200),
            [0.0, 1.0, 0.0, 1.0],
            1.0,
        );
        p.translate(50, 5.0);
        if let DrawingKind::Position {
            entry, end_time, ..
        } = p.kind
        {
            assert_eq!(entry, (150, 50005.0));
            assert_eq!(end_time, Some(250));
        } else {
            panic!("Expected Position");
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

    #[test]
    fn test_position_style_serialization_and_backward_compatibility() {
        // 1. New style serialized and deserialized
        let mut pos = Drawing::position(
            (1000, 50000.0),
            49000.0,
            53000.0,
            true,
            Some(2000),
            [0.2, 0.8, 0.4, 1.0],
            1.5,
        );
        pos.set_position_profit_color(Some([0.1, 0.9, 0.5, 0.4]));
        pos.set_position_stop_color(Some([0.9, 0.2, 0.1, 0.5]));
        let serialized = serde_json::to_string(&pos).unwrap();
        let deserialized: Drawing = serde_json::from_str(&serialized).unwrap();
        assert_eq!(pos, deserialized);
        assert_eq!(
            deserialized.position_style().profit_color,
            Some([0.1, 0.9, 0.5, 0.4])
        );
        assert_eq!(
            deserialized.position_style().stop_color,
            Some([0.9, 0.2, 0.1, 0.5])
        );
        assert!(deserialized.position_style().show_price_path);

        // 2. Backward compatibility: legacy Position JSON without style and without end_time
        let legacy_json = r#"{"id":"11111111-1111-1111-1111-111111111111","kind":{"Position":{"entry":[1000,50000.0],"stop_price":49000.0,"target_price":53000.0,"is_long":true}},"color":[0.2,0.8,0.4,1.0],"width":1.0,"is_selected":false,"is_locked":false}"#;
        let leg_pos: Drawing = serde_json::from_str(legacy_json).unwrap();
        let style = leg_pos.position_style();
        assert_eq!(style, PositionStyle::default());
        assert_eq!(style.profit_color, None);
        assert_eq!(style.stop_color, None);
        assert_eq!(
            style.resolved_profit_color(default_profit_color()),
            [0.12, 0.78, 0.42, 0.1]
        );
        assert_eq!(
            style.resolved_stop_color(default_stop_color()),
            [0.88, 0.24, 0.24, 0.1]
        );
        if let DrawingKind::Position { end_time, .. } = leg_pos.kind {
            assert_eq!(end_time, None);
        } else {
            panic!("Expected Position");
        }

        // 3. Backward compatibility: legacy Position with explicit profit_color and stop_color array
        let legacy_colored = r#"{"id":"11111111-1111-1111-1111-111111111111","kind":{"Position":{"entry":[1000,50000.0],"stop_price":49000.0,"target_price":53000.0,"is_long":true,"style":{"profit_color":[0.12,0.78,0.42,0.2],"stop_color":[0.88,0.24,0.24,0.2],"entry_color":[0.85,0.9,0.95,0.9],"show_price_path":true}}},"color":[0.2,0.8,0.4,1.0],"width":1.0,"is_selected":false,"is_locked":false}"#;
        let leg_colored_pos: Drawing = serde_json::from_str(legacy_colored).unwrap();
        assert_eq!(
            leg_colored_pos.position_style().profit_color,
            Some([0.12, 0.78, 0.42, 0.2])
        );
        assert_eq!(
            leg_colored_pos.position_style().stop_color,
            Some([0.88, 0.24, 0.24, 0.2])
        );
    }

    #[test]
    fn test_drawing_store_operations() {
        let sym = "TEST_DRAWING_SYM";
        let d1 = Drawing::horizontal(65000.0, [1.0, 0.5, 0.0, 1.0], 2.0);
        let id1 = d1.id;
        let mut d2 = Drawing::trendline((100, 10.0), (200, 20.0), [0.0, 1.0, 0.0, 1.0], 1.5);
        d2.is_locked = true;
        let id2 = d2.id;

        DrawingStore::add(sym, d1);
        DrawingStore::add(sym, d2);

        let drawings = DrawingStore::for_symbol(sym);
        assert_eq!(drawings.len(), 2);
        assert!(drawings.iter().any(|d| d.id == id1));
        assert!(drawings.iter().any(|d| d.id == id2 && d.is_locked));

        // Update d1 price
        let mut updated_d1 = Drawing::horizontal(66000.0, [1.0, 0.5, 0.0, 1.0], 2.0);
        updated_d1.id = id1;
        DrawingStore::update(sym, updated_d1);
        let drawings_after_update = DrawingStore::for_symbol(sym);
        let found = drawings_after_update.iter().find(|d| d.id == id1).unwrap();
        if let DrawingKind::HorizontalLine { price } = found.kind {
            assert_eq!(price, 66000.0);
        } else {
            panic!("Expected horizontal line");
        }

        // Clear unlocked drawings
        DrawingStore::clear_for_symbol(sym);
        let drawings_after_clear = DrawingStore::for_symbol(sym);
        assert_eq!(drawings_after_clear.len(), 1);
        assert_eq!(drawings_after_clear[0].id, id2);

        // Remove locked drawing directly
        DrawingStore::remove(id2);
        assert!(DrawingStore::for_symbol(sym).is_empty());
    }

    #[test]
    fn test_drawing_pane_isolation_and_sync() {
        let sym = "TEST_PANE_SYM";
        let pane_1 = uuid::Uuid::new_v4();
        let pane_2 = uuid::Uuid::new_v4();

        // 1. Drawing created on pane 1 is unsynced by default
        let mut d1 = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
        d1.pane_id = Some(pane_1);
        d1.is_synced = false;
        let id1 = d1.id;
        DrawingStore::add(sym, d1.clone());

        // Pane 1 can see it
        let for_p1 = DrawingStore::for_symbol_and_pane(sym, pane_1);
        assert_eq!(for_p1.len(), 1);
        assert_eq!(for_p1[0].id, id1);

        // Pane 2 cannot see it
        let for_p2 = DrawingStore::for_symbol_and_pane(sym, pane_2);
        assert_eq!(for_p2.len(), 0);

        // 2. Toggle sync to true on d1
        d1.is_synced = true;
        DrawingStore::update(sym, d1.clone());

        // Now both panes see it
        assert_eq!(DrawingStore::for_symbol_and_pane(sym, pane_1).len(), 1);
        assert_eq!(DrawingStore::for_symbol_and_pane(sym, pane_2).len(), 1);

        // 3. Add unsynced drawing on pane 2
        let mut d2 = Drawing::trendline((10, 10.0), (20, 20.0), [0.0, 1.0, 0.0, 1.0], 1.0);
        d2.pane_id = Some(pane_2);
        d2.is_synced = false;
        let id2 = d2.id;
        DrawingStore::add(sym, d2);

        // Pane 1 sees synced d1 only (len = 1)
        assert_eq!(DrawingStore::for_symbol_and_pane(sym, pane_1).len(), 1);
        // Pane 2 sees synced d1 AND its local d2 (len = 2)
        assert_eq!(DrawingStore::for_symbol_and_pane(sym, pane_2).len(), 2);

        // 4. Clearing on pane 2 removes d2 and synced d1, but preserves pane 1 if pane 1 had its own local
        DrawingStore::clear_for_symbol_and_pane(sym, pane_2);
        assert_eq!(DrawingStore::for_symbol_and_pane(sym, pane_2).len(), 0);

        // Cleanup
        DrawingStore::remove(id1);
        DrawingStore::remove(id2);
    }
}
