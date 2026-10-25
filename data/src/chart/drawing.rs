use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl std::fmt::Display for LineStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineStyle::Solid => write!(f, "Solid"),
            LineStyle::Dashed => write!(f, "Dashed"),
            LineStyle::Dotted => write!(f, "Dotted"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlignH {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlignV {
    #[default]
    Top,
    Center,
    Bottom,
}

pub fn default_annotation_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

pub fn default_annotation_size() -> f32 {
    12.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingAnnotation {
    pub text: String,
    #[serde(default = "default_annotation_color")]
    pub color: [f32; 4],
    #[serde(default = "default_annotation_size")]
    pub font_size: f32,
    #[serde(default)]
    pub align_h: TextAlignH,
    #[serde(default)]
    pub align_v: TextAlignV,
}

impl Default for DrawingAnnotation {
    fn default() -> Self {
        Self {
            text: String::new(),
            color: default_annotation_color(),
            font_size: default_annotation_size(),
            align_h: TextAlignH::default(),
            align_v: TextAlignV::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingVisibility {
    #[serde(default = "default_true")]
    pub seconds: bool,
    #[serde(default = "default_true")]
    pub minutes: bool,
    #[serde(default = "default_true")]
    pub hours: bool,
    #[serde(default = "default_true")]
    pub days: bool,
    #[serde(default = "default_true")]
    pub weeks: bool,
}

impl Default for DrawingVisibility {
    fn default() -> Self {
        Self {
            seconds: true,
            minutes: true,
            hours: true,
            days: true,
            weeks: true,
        }
    }
}

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
    #[serde(default)]
    pub layout_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub line_style: LineStyle,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default)]
    pub extend_right: bool,
    #[serde(default)]
    pub arrow_left: bool,
    #[serde(default)]
    pub arrow_right: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<DrawingAnnotation>,
    #[serde(default)]
    pub visibility: DrawingVisibility,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::HorizontalLine { price: 0.0 },
            color: [1.0, 1.0, 1.0, 1.0],
            width: 1.0,
            is_selected: false,
            is_locked: false,
            is_synced: false,
            pane_id: None,
            layout_id: None,
            line_style: LineStyle::Solid,
            extend_left: false,
            extend_right: false,
            arrow_left: false,
            arrow_right: false,
            fill_opacity: None,
            annotation: None,
            visibility: DrawingVisibility::default(),
        }
    }
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
            ..Default::default()
        }
    }

    pub fn trendline(p1: (u64, f32), p2: (u64, f32), color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Trendline { p1, p2 },
            color,
            width,
            ..Default::default()
        }
    }

    pub fn rectangle(p1: (u64, f32), p2: (u64, f32), color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Rectangle { p1, p2 },
            color,
            width,
            ..Default::default()
        }
    }

    pub fn brush(points: Vec<(u64, f32)>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Brush { points },
            color,
            width,
            ..Default::default()
        }
    }

    pub fn path(points: Vec<(u64, f32)>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind: DrawingKind::Path { points },
            color,
            width,
            ..Default::default()
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
            ..Default::default()
        }
    }

    pub fn is_visible_for_timeframe(&self, timeframe_ms: u64) -> bool {
        if timeframe_ms < 60_000 {
            self.visibility.seconds
        } else if timeframe_ms < 3_600_000 {
            self.visibility.minutes
        } else if timeframe_ms < 86_400_000 {
            self.visibility.hours
        } else if timeframe_ms < 7 * 86_400_000 {
            self.visibility.days
        } else {
            self.visibility.weeks
        }
    }

    pub fn with_layout_id(mut self, layout_id: uuid::Uuid) -> Self {
        self.layout_id = Some(layout_id);
        self
    }

    pub fn with_pane_id(mut self, pane_id: uuid::Uuid) -> Self {
        self.pane_id = Some(pane_id);
        self
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

    pub fn apply_template(&mut self, template: &DrawingTemplate) {
        self.color = template.color;
        self.width = template.width;
        if let Some(pos_style) = template.position_style
            && let DrawingKind::Position { ref mut style, .. } = self.kind
        {
            *style = Some(pos_style);
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingRecord {
    pub ticker_symbol: String,
    pub drawing: Drawing,
    #[serde(default)]
    pub layout_id: Option<uuid::Uuid>,
}

static GLOBAL_DRAWINGS: LazyLock<RwLock<Vec<DrawingRecord>>> =
    LazyLock::new(|| RwLock::new(crate::load_drawings()));

static DRAWING_SAVE_LOCK: Mutex<()> = Mutex::new(());
static DRAWING_SAVE_VERSION: AtomicU64 = AtomicU64::new(0);
static DRAWING_LAST_WRITTEN_VERSION: AtomicU64 = AtomicU64::new(0);

#[inline]
fn read_drawings() -> std::sync::RwLockReadGuard<'static, Vec<DrawingRecord>> {
    GLOBAL_DRAWINGS.read().unwrap_or_else(|p| p.into_inner())
}

#[inline]
fn write_drawings() -> std::sync::RwLockWriteGuard<'static, Vec<DrawingRecord>> {
    GLOBAL_DRAWINGS.write().unwrap_or_else(|p| p.into_inner())
}

fn spawn_save_drawings(snapshot: Vec<DrawingRecord>) {
    let version = DRAWING_SAVE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        let _guard = DRAWING_SAVE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if version < DRAWING_LAST_WRITTEN_VERSION.load(Ordering::Relaxed) {
            return;
        }
        let _ = crate::save_drawings(&snapshot);
        DRAWING_LAST_WRITTEN_VERSION.store(version, Ordering::Relaxed);
    });
}

pub struct DrawingStore;

impl DrawingStore {
    /// Retrieve a clone of all drawing records
    pub fn all() -> Vec<DrawingRecord> {
        read_drawings().clone()
    }

    /// Retrieve all drawings for a specific ticker symbol
    pub fn for_symbol(symbol: &str) -> Vec<Drawing> {
        read_drawings()
            .iter()
            .filter(|r| r.ticker_symbol == symbol)
            .map(|r| r.drawing.clone())
            .collect()
    }

    /// Retrieve drawings for a specific ticker symbol visible to a given pane.
    /// Returns drawings that are synced to all charts, drawings specific to this pane,
    /// or legacy drawings that have no pane_id assigned yet.
    pub fn for_symbol_and_pane(symbol: &str, pane_id: uuid::Uuid) -> Vec<Drawing> {
        read_drawings()
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

    /// Retrieve drawings for a specific ticker symbol visible to a given layout and pane.
    /// Returns drawings that are globally synced, or drawings matching this layout_id and pane_id.
    pub fn for_symbol_layout_and_pane(
        symbol: &str,
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
    ) -> Vec<Drawing> {
        read_drawings()
            .iter()
            .filter(|r| {
                if r.ticker_symbol != symbol {
                    return false;
                }
                if r.drawing.is_synced {
                    return true;
                }
                let effective_layout = r.layout_id.or(r.drawing.layout_id);
                let layout_matches = match effective_layout {
                    Some(lid) => lid == layout_id,
                    None => true,
                };
                let pane_matches = match r.drawing.pane_id {
                    Some(pid) => pid == pane_id,
                    None => true,
                };
                layout_matches && pane_matches
            })
            .map(|r| r.drawing.clone())
            .collect()
    }

    /// Add or update a drawing for a symbol and persist to disk asynchronously
    pub fn add(symbol: impl Into<String>, drawing: Drawing) {
        let snapshot = {
            let mut drawings = write_drawings();
            if let Some(existing) = drawings.iter_mut().find(|r| r.drawing.id == drawing.id) {
                existing.drawing = drawing.clone();
                existing.layout_id = drawing.layout_id;
            } else {
                drawings.push(DrawingRecord {
                    ticker_symbol: symbol.into(),
                    layout_id: drawing.layout_id,
                    drawing,
                });
            }
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Update an existing drawing for a symbol and persist to disk asynchronously
    pub fn update(symbol: &str, drawing: Drawing) {
        let snapshot = {
            let mut drawings = write_drawings();
            if let Some(record) = drawings.iter_mut().find(|r| r.drawing.id == drawing.id) {
                record.drawing = drawing.clone();
                record.ticker_symbol = symbol.to_string();
                record.layout_id = drawing.layout_id;
            } else {
                drawings.push(DrawingRecord {
                    ticker_symbol: symbol.to_string(),
                    layout_id: drawing.layout_id,
                    drawing,
                });
            }
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Remove a drawing by ID and persist to disk asynchronously
    pub fn remove(id: uuid::Uuid) {
        let snapshot = {
            let mut drawings = write_drawings();
            drawings.retain(|r| r.drawing.id != id);
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Clear unlocked drawings for a specific symbol and persist to disk asynchronously
    pub fn clear_for_symbol(symbol: &str) {
        let snapshot = {
            let mut drawings = write_drawings();
            drawings.retain(|r| r.ticker_symbol != symbol || r.drawing.is_locked);
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Clear unlocked drawings for a specific symbol visible to a given pane.
    /// Drawings belonging exclusively to another pane remain untouched.
    pub fn clear_for_symbol_and_pane(symbol: &str, pane_id: uuid::Uuid) {
        let snapshot = {
            let mut drawings = write_drawings();
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
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Clear unlocked drawings for a specific symbol visible to a given layout and pane.
    /// Drawings belonging exclusively to another layout or pane remain untouched.
    pub fn clear_for_symbol_layout_and_pane(
        symbol: &str,
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
    ) {
        let snapshot = {
            let mut drawings = write_drawings();
            drawings.retain(|r| {
                if r.ticker_symbol != symbol || r.drawing.is_locked {
                    return true;
                }
                if r.drawing.is_synced {
                    return false;
                }
                let effective_layout = r.layout_id.or(r.drawing.layout_id);
                if let Some(lid) = effective_layout
                    && lid != layout_id
                {
                    return true;
                }
                if let Some(pid) = r.drawing.pane_id
                    && pid != pane_id
                {
                    return true;
                }
                false
            });
            drawings.clone()
        };
        spawn_save_drawings(snapshot);
    }

    /// Force save all drawings to disk asynchronously
    pub fn save() {
        let snapshot = read_drawings().clone();
        spawn_save_drawings(snapshot);
    }

    /// Force synchronous save of all drawings to disk
    pub fn save_sync() -> std::io::Result<()> {
        let snapshot = read_drawings().clone();
        crate::save_drawings(&snapshot)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingTemplate {
    pub id: uuid::Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<DrawingTool>,
    pub color: [f32; 4],
    pub width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_style: Option<PositionStyle>,
    #[serde(default)]
    pub is_builtin: bool,
}

impl DrawingTemplate {
    pub fn new(name: impl Into<String>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            tool: None,
            color,
            width,
            position_style: None,
            is_builtin: false,
        }
    }

    pub fn with_tool(mut self, tool: DrawingTool) -> Self {
        self.tool = Some(tool);
        self
    }

    pub fn with_position_style(mut self, style: PositionStyle) -> Self {
        self.position_style = Some(style);
        self
    }

    pub fn builtin(name: impl Into<String>, color: [f32; 4], width: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            tool: None,
            color,
            width,
            position_style: None,
            is_builtin: true,
        }
    }

    pub fn builtin_presets() -> Vec<Self> {
        vec![
            Self::builtin("Cyan Accent", [0.0, 0.85, 1.0, 1.0], 2.0),
            Self::builtin("Key Level", [1.0, 0.84, 0.0, 1.0], 2.0),
            Self::builtin("Major Alert", [0.94, 0.27, 0.24, 1.0], 3.0),
            Self::builtin("Faint Guide", [0.7, 0.7, 0.7, 0.4], 1.0),
            Self::builtin("Accent Zone", [0.67, 0.28, 0.74, 0.8], 2.0),
        ]
    }
}

static GLOBAL_DRAWING_TEMPLATES: LazyLock<RwLock<Vec<DrawingTemplate>>> = LazyLock::new(|| {
    let mut loaded = crate::load_drawing_templates();
    if loaded.is_empty() {
        loaded = DrawingTemplate::builtin_presets();
    }
    RwLock::new(loaded)
});

static TEMPLATE_SAVE_LOCK: Mutex<()> = Mutex::new(());
static TEMPLATE_SAVE_VERSION: AtomicU64 = AtomicU64::new(0);
static TEMPLATE_LAST_WRITTEN_VERSION: AtomicU64 = AtomicU64::new(0);

#[inline]
fn read_templates() -> std::sync::RwLockReadGuard<'static, Vec<DrawingTemplate>> {
    GLOBAL_DRAWING_TEMPLATES
        .read()
        .unwrap_or_else(|p| p.into_inner())
}

#[inline]
fn write_templates() -> std::sync::RwLockWriteGuard<'static, Vec<DrawingTemplate>> {
    GLOBAL_DRAWING_TEMPLATES
        .write()
        .unwrap_or_else(|p| p.into_inner())
}

fn spawn_save_templates(snapshot: Vec<DrawingTemplate>) {
    let version = TEMPLATE_SAVE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        let _guard = TEMPLATE_SAVE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if version < TEMPLATE_LAST_WRITTEN_VERSION.load(Ordering::Relaxed) {
            return;
        }
        let _ = crate::save_drawing_templates(&snapshot);
        TEMPLATE_LAST_WRITTEN_VERSION.store(version, Ordering::Relaxed);
    });
}

pub struct DrawingTemplateStore;

impl DrawingTemplateStore {
    /// Retrieve a clone of all drawing templates (presets + user defined)
    pub fn all() -> Vec<DrawingTemplate> {
        read_templates().clone()
    }

    /// Retrieve drawing templates matching a specific tool (or general templates where tool is None)
    pub fn for_tool(tool: Option<DrawingTool>) -> Vec<DrawingTemplate> {
        read_templates()
            .iter()
            .filter(|t| t.tool.is_none() || tool.is_none() || t.tool == tool)
            .cloned()
            .collect()
    }

    /// Add or update a template and persist to disk asynchronously
    pub fn add(template: DrawingTemplate) {
        let snapshot = {
            let mut list = write_templates();
            if let Some(existing) = list.iter_mut().find(|t| t.id == template.id) {
                *existing = template;
            } else {
                list.push(template);
            }
            list.clone()
        };
        spawn_save_templates(snapshot);
    }

    /// Remove a template by ID and persist to disk asynchronously
    pub fn remove(id: uuid::Uuid) {
        let snapshot = {
            let mut list = write_templates();
            list.retain(|t| t.id != id);
            list.clone()
        };
        spawn_save_templates(snapshot);
    }

    /// Reset templates to built-in presets and persist to disk asynchronously
    pub fn reset_to_defaults() {
        let snapshot = {
            let mut list = write_templates();
            *list = DrawingTemplate::builtin_presets();
            list.clone()
        };
        spawn_save_templates(snapshot);
    }

    /// Force save all templates to disk asynchronously
    pub fn save() {
        let snapshot = read_templates().clone();
        spawn_save_templates(snapshot);
    }

    /// Force synchronous save of all templates to disk
    pub fn save_sync() -> std::io::Result<()> {
        let snapshot = read_templates().clone();
        crate::save_drawing_templates(&snapshot)
    }
}

static RECENT_COLORS: LazyLock<RwLock<Vec<[f32; 4]>>> = LazyLock::new(|| {
    RwLock::new(vec![
        [1.0, 1.0, 1.0, 1.0],
        [0.65, 0.65, 0.65, 1.0],
        [0.94, 0.27, 0.24, 1.0],
        [1.0, 0.60, 0.0, 1.0],
        [1.0, 0.92, 0.23, 1.0],
        [0.15, 0.68, 0.38, 1.0],
        [0.16, 0.38, 1.0, 1.0],
        [0.67, 0.28, 0.74, 1.0],
    ])
});

pub fn get_recent_colors() -> Vec<[f32; 4]> {
    RECENT_COLORS
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

pub fn push_recent_color(color: [f32; 4]) {
    let mut colors = RECENT_COLORS.write().unwrap_or_else(|p| p.into_inner());
    colors.retain(|c| {
        (c[0] - color[0]).abs() > 0.01
            || (c[1] - color[1]).abs() > 0.01
            || (c[2] - color[2]).abs() > 0.01
            || (c[3] - color[3]).abs() > 0.01
    });
    colors.insert(0, color);
    if colors.len() > 16 {
        colors.truncate(16);
    }
}

pub fn color_to_hex(color: [f32; 4]) -> String {
    let r = (color[0] * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (color[1] * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (color[2] * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = (color[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    if a == 255 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
    }
}

pub fn hex_to_color(hex: &str) -> Option<[f32; 4]> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        Some([r, g, b, 1.0])
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        let a = u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0;
        Some([r, g, b, a])
    } else {
        None
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
        let sym_owned = format!("TEST_DRAWING_SYM_{}", uuid::Uuid::new_v4());
        let sym = sym_owned.as_str();
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

    #[test]
    fn test_dat06_drawing_store_lock_poisoning_resilience() {
        use std::panic;

        let _ = panic::catch_unwind(|| {
            let _guard = GLOBAL_DRAWINGS.write().unwrap_or_else(|p| p.into_inner());
            panic!("simulated panic while holding GLOBAL_DRAWINGS write lock");
        });

        assert!(GLOBAL_DRAWINGS.is_poisoned());

        let sym = "POISON_TEST_DRAWING";
        let d = Drawing::horizontal(100.0, [1.0, 0.0, 0.0, 1.0], 1.0);
        let id = d.id;

        DrawingStore::add(sym, d);
        let drawings = DrawingStore::for_symbol(sym);
        assert!(drawings.iter().any(|item| item.id == id));

        DrawingStore::remove(id);
        assert!(
            !DrawingStore::for_symbol(sym)
                .iter()
                .any(|item| item.id == id)
        );
    }

    #[test]
    fn test_drawing_template_application() {
        let mut d = Drawing::horizontal(100.0, [1.0, 1.0, 1.0, 1.0], 1.0);
        let template = DrawingTemplate::builtin("Test Preset", [0.0, 0.85, 1.0, 1.0], 3.0);
        d.apply_template(&template);
        assert_eq!(d.color, [0.0, 0.85, 1.0, 1.0]);
        assert_eq!(d.width, 3.0);
    }

    #[test]
    fn test_drawing_template_application_to_position() {
        let mut pos = Drawing::position(
            (100, 50000.0),
            49000.0,
            53000.0,
            true,
            Some(200),
            [1.0, 1.0, 1.0, 1.0],
            1.0,
        );
        let custom_pos_style = PositionStyle {
            profit_color: Some([0.1, 0.9, 0.2, 0.5]),
            stop_color: Some([0.9, 0.1, 0.2, 0.5]),
            entry_color: [0.0, 0.5, 1.0, 1.0],
            show_price_path: false,
        };
        let template = DrawingTemplate::new("Pos Preset", [0.2, 0.8, 0.4, 1.0], 2.5)
            .with_tool(DrawingTool::LongPosition)
            .with_position_style(custom_pos_style);

        pos.apply_template(&template);
        assert_eq!(pos.color, [0.2, 0.8, 0.4, 1.0]);
        assert_eq!(pos.width, 2.5);
        assert_eq!(pos.position_style(), custom_pos_style);
    }

    #[test]
    fn test_drawing_template_store_crud_and_serialization() {
        let t1 = DrawingTemplate::new("Custom Scalp", [1.0, 0.5, 0.0, 1.0], 2.0);
        let id1 = t1.id;
        DrawingTemplateStore::add(t1);

        let all = DrawingTemplateStore::all();
        assert!(all.iter().any(|t| t.id == id1));

        let for_none = DrawingTemplateStore::for_tool(None);
        assert!(for_none.iter().any(|t| t.id == id1));

        let for_kline = DrawingTemplateStore::for_tool(Some(DrawingTool::Trendline));
        assert!(for_kline.iter().any(|t| t.id == id1));

        DrawingTemplateStore::remove(id1);
        let after_remove = DrawingTemplateStore::all();
        assert!(!after_remove.iter().any(|t| t.id == id1));
    }

    #[test]
    fn test_dat17_drawing_isolation_per_layout() {
        let sym_owned = format!("TEST_LAYOUT_ISO_{}", uuid::Uuid::new_v4());
        let sym = sym_owned.as_str();

        let layout_1 = uuid::Uuid::new_v4();
        let layout_2 = uuid::Uuid::new_v4();

        let pane_1 = uuid::Uuid::new_v4();
        let pane_2 = uuid::Uuid::new_v4();

        // 1. Add drawing local to layout 1
        let mut d_layout1 = Drawing::horizontal(50000.0, [1.0, 0.0, 0.0, 1.0], 2.0);
        d_layout1.layout_id = Some(layout_1);
        d_layout1.pane_id = Some(pane_1);
        let id_l1 = d_layout1.id;
        DrawingStore::add(sym, d_layout1);

        // 2. Add drawing local to layout 2
        let mut d_layout2 = Drawing::horizontal(60000.0, [0.0, 1.0, 0.0, 1.0], 2.0);
        d_layout2.layout_id = Some(layout_2);
        d_layout2.pane_id = Some(pane_2);
        let id_l2 = d_layout2.id;
        DrawingStore::add(sym, d_layout2);

        // 3. Add globally synced drawing
        let mut d_synced = Drawing::horizontal(70000.0, [0.0, 0.0, 1.0, 1.0], 2.0);
        d_synced.is_synced = true;
        let id_synced = d_synced.id;
        DrawingStore::add(sym, d_synced);

        // Verify layout 1 sees d_layout1 and d_synced (2 total)
        let l1_drawings = DrawingStore::for_symbol_layout_and_pane(sym, layout_1, pane_1);
        assert_eq!(l1_drawings.len(), 2);
        assert!(l1_drawings.iter().any(|d| d.id == id_l1));
        assert!(l1_drawings.iter().any(|d| d.id == id_synced));
        assert!(!l1_drawings.iter().any(|d| d.id == id_l2));

        // Verify layout 2 sees d_layout2 and d_synced (2 total)
        let l2_drawings = DrawingStore::for_symbol_layout_and_pane(sym, layout_2, pane_2);
        assert_eq!(l2_drawings.len(), 2);
        assert!(l2_drawings.iter().any(|d| d.id == id_l2));
        assert!(l2_drawings.iter().any(|d| d.id == id_synced));
        assert!(!l2_drawings.iter().any(|d| d.id == id_l1));

        // Verify even with collision of pane_id across layouts, layout_id isolates them:
        let l2_collision = DrawingStore::for_symbol_layout_and_pane(sym, layout_2, pane_1);
        assert_eq!(l2_collision.len(), 1); // Only d_synced, d_layout1 is filtered out by layout_id
        assert_eq!(l2_collision[0].id, id_synced);

        // 4. Test clear_for_symbol_layout_and_pane in layout 1
        DrawingStore::clear_for_symbol_layout_and_pane(sym, layout_1, pane_1);
        let l1_after_clear = DrawingStore::for_symbol_layout_and_pane(sym, layout_1, pane_1);
        assert_eq!(l1_after_clear.len(), 0);

        // Layout 2 local drawing is preserved untouched!
        let l2_after_clear = DrawingStore::for_symbol_layout_and_pane(sym, layout_2, pane_2);
        assert_eq!(l2_after_clear.len(), 1);
        assert_eq!(l2_after_clear[0].id, id_l2);

        // Cleanup
        DrawingStore::remove(id_l1);
        DrawingStore::remove(id_l2);
        DrawingStore::remove(id_synced);
    }

    #[test]
    fn test_ter36_advanced_drawing_properties_serialization_and_backward_compatibility() {
        // 1. Serialization with all new properties
        let mut d = Drawing::trendline((1000, 100.0), (2000, 200.0), [0.1, 0.2, 0.3, 0.9], 2.0);
        d.line_style = LineStyle::Dashed;
        d.extend_left = true;
        d.extend_right = false;
        d.arrow_left = false;
        d.arrow_right = true;
        d.fill_opacity = Some(0.4);
        d.annotation = Some(DrawingAnnotation {
            text: "Weekly Open".to_string(),
            color: [1.0, 0.9, 0.8, 1.0],
            font_size: 14.0,
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Top,
        });
        d.visibility.seconds = false;
        d.visibility.weeks = true;

        let json = serde_json::to_string(&d).unwrap();
        let deserialized: Drawing = serde_json::from_str(&json).unwrap();
        assert_eq!(d, deserialized);
        assert_eq!(deserialized.line_style, LineStyle::Dashed);
        assert!(deserialized.extend_left);
        assert!(!deserialized.extend_right);
        assert!(deserialized.arrow_right);
        assert_eq!(deserialized.fill_opacity, Some(0.4));
        assert_eq!(
            deserialized.annotation.as_ref().unwrap().text,
            "Weekly Open"
        );
        assert!(!deserialized.is_visible_for_timeframe(1_000)); // seconds = false
        assert!(deserialized.is_visible_for_timeframe(7 * 86_400_000)); // weeks = true

        // 2. Backward compatibility: legacy JSON without new fields
        let legacy_json = r#"{"id":"00000000-0000-0000-0000-000000000001","kind":{"HorizontalLine":{"price":50000.0}},"color":[1.0,0.5,0.0,1.0],"width":2.0,"is_selected":false,"is_locked":false}"#;
        let leg_d: Drawing = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(leg_d.line_style, LineStyle::Solid);
        assert!(!leg_d.extend_left);
        assert!(!leg_d.extend_right);
        assert!(!leg_d.arrow_left);
        assert!(!leg_d.arrow_right);
        assert_eq!(leg_d.fill_opacity, None);
        assert_eq!(leg_d.annotation, None);
        assert!(leg_d.visibility.seconds);
        assert!(leg_d.visibility.minutes);
        assert!(leg_d.visibility.hours);
        assert!(leg_d.visibility.days);
        assert!(leg_d.visibility.weeks);
    }

    #[test]
    fn test_ter36_hex_color_conversion_and_recent_swatches() {
        assert_eq!(color_to_hex([1.0, 1.0, 1.0, 1.0]), "#FFFFFF");
        assert_eq!(color_to_hex([0.0, 0.0, 0.0, 1.0]), "#000000");
        assert_eq!(color_to_hex([1.0, 0.0, 0.0, 0.5]), "#FF000080");

        let c1 = hex_to_color("#FFFFFF").unwrap();
        assert!((c1[0] - 1.0).abs() < 1e-3);
        assert!((c1[1] - 1.0).abs() < 1e-3);
        assert!((c1[2] - 1.0).abs() < 1e-3);
        assert!((c1[3] - 1.0).abs() < 1e-3);

        let c2 = hex_to_color("#123456").unwrap();
        assert_eq!(color_to_hex(c2), "#123456");

        let c3 = hex_to_color("12345678").unwrap();
        assert_eq!(color_to_hex(c3), "#12345678");

        assert!(hex_to_color("INVALID").is_none());

        push_recent_color([0.2, 0.4, 0.6, 1.0]);
        let recents = get_recent_colors();
        assert!((recents[0][0] - 0.2).abs() < 1e-3);
        assert!((recents[0][1] - 0.4).abs() < 1e-3);
    }
}
