use crate::layout::{Layout, WindowSpec, pane::Pane};
use crate::{AudioStream, Theme, UserTimezone, tickers_table};
use serde::{Deserialize, Serialize};

pub const MAX_BUNDLE_PAYLOAD_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_PANE_DEPTH: usize = 16;
pub const MAX_PANES_PER_LAYOUT: usize = 64;
pub const MAX_POPOUTS_PER_LAYOUT: usize = 16;
pub const MAX_LAYOUTS_PER_BUNDLE: usize = 50;
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportType {
    Layout,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BundlePayload {
    Layout(Layout),
    Workspace(WorkspaceBundle),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceBundle {
    pub layouts: Vec<Layout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_theme: Option<Theme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<UserTimezone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tickers_table: Option<tickers_table::Settings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_cfg: Option<AudioStream>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_in_quote_ccy: Option<exchange::SizeUnit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_kline_config: Option<crate::chart::kline::Config>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBundle {
    pub schema_version: u32,
    pub export_type: ExportType,
    pub exported_at: i64,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BundleMetadata>,
    pub payload: BundlePayload,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum BundleValidationError {
    #[error("Payload exceeds maximum size limit of {0} bytes")]
    PayloadTooLarge(usize),
    #[error("Failed to parse JSON: {0}")]
    JsonError(String),
    #[error("Unsupported schema version: {0} (max supported: {1})")]
    UnsupportedVersion(u32, u32),
    #[error("Pane tree recursion exceeds maximum depth of {0}")]
    PaneTreeTooDeep(usize),
    #[error("Layout exceeds maximum pane limit of {0}")]
    TooManyPanes(usize),
    #[error("Layout exceeds maximum popouts limit of {0}")]
    TooManyPopouts(usize),
    #[error("Bundle exceeds maximum layout limit of {0}")]
    TooManyLayouts(usize),
    #[error("Empty bundle: no layouts found")]
    EmptyBundle,
}

pub fn sanitize_window_spec(spec: &mut WindowSpec) {
    if !spec.width.is_finite() || spec.width < 100.0 || spec.width > 10000.0 {
        spec.width = 1024.0;
    }
    if !spec.height.is_finite() || spec.height < 100.0 || spec.height > 10000.0 {
        spec.height = 768.0;
    }
    if !spec.pos_x.is_finite() || spec.pos_x < -10000.0 || spec.pos_x > 50000.0 {
        spec.pos_x = 0.0;
    }
    if !spec.pos_y.is_finite() || spec.pos_y < -10000.0 || spec.pos_y > 50000.0 {
        spec.pos_y = 0.0;
    }
}

pub fn validate_and_sanitize_pane(
    pane: &mut Pane,
    depth: usize,
    pane_count: &mut usize,
) -> Result<(), BundleValidationError> {
    *pane_count += 1;
    if *pane_count > MAX_PANES_PER_LAYOUT {
        return Err(BundleValidationError::TooManyPanes(MAX_PANES_PER_LAYOUT));
    }
    if depth > MAX_PANE_DEPTH {
        return Err(BundleValidationError::PaneTreeTooDeep(MAX_PANE_DEPTH));
    }

    if let Pane::Split { ratio, a, b, .. } = pane {
        if !ratio.is_finite() || *ratio < 0.05 || *ratio > 0.95 {
            *ratio = 0.5;
        }
        validate_and_sanitize_pane(a, depth + 1, pane_count)?;
        validate_and_sanitize_pane(b, depth + 1, pane_count)?;
    }
    Ok(())
}

pub fn sanitize_layout(layout: &mut Layout) -> Result<(), BundleValidationError> {
    let sanitized_name = layout
        .name
        .chars()
        .filter(|c| !c.is_control())
        .take(30)
        .collect::<String>()
        .trim()
        .to_string();

    layout.name = if sanitized_name.is_empty() {
        "Imported Layout".to_string()
    } else {
        sanitized_name
    };

    let mut pane_count = 0;
    validate_and_sanitize_pane(&mut layout.dashboard.pane, 1, &mut pane_count)?;

    if layout.dashboard.popout.len() > MAX_POPOUTS_PER_LAYOUT {
        return Err(BundleValidationError::TooManyPopouts(
            MAX_POPOUTS_PER_LAYOUT,
        ));
    }

    for (popout_pane, spec) in &mut layout.dashboard.popout {
        let mut popout_count = 0;
        validate_and_sanitize_pane(popout_pane, 1, &mut popout_count)?;
        sanitize_window_spec(spec);
    }

    Ok(())
}

impl ConfigBundle {
    pub fn new_layout(layout: Layout, metadata: Option<BundleMetadata>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            export_type: ExportType::Layout,
            exported_at: chrono::Utc::now().timestamp(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            metadata,
            payload: BundlePayload::Layout(layout),
        }
    }

    pub fn new_workspace(bundle: WorkspaceBundle, metadata: Option<BundleMetadata>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            export_type: ExportType::Workspace,
            exported_at: chrono::Utc::now().timestamp(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            metadata,
            payload: BundlePayload::Workspace(bundle),
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn to_json_compact(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(raw: &str) -> Result<Self, BundleValidationError> {
        if raw.len() > MAX_BUNDLE_PAYLOAD_BYTES {
            return Err(BundleValidationError::PayloadTooLarge(
                MAX_BUNDLE_PAYLOAD_BYTES,
            ));
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(BundleValidationError::EmptyBundle);
        }

        // 1. Attempt standard ConfigBundle deserialization
        if let Ok(mut bundle) = serde_json::from_str::<ConfigBundle>(trimmed) {
            if bundle.schema_version > CURRENT_SCHEMA_VERSION {
                return Err(BundleValidationError::UnsupportedVersion(
                    bundle.schema_version,
                    CURRENT_SCHEMA_VERSION,
                ));
            }
            bundle.sanitize()?;
            return Ok(bundle);
        }

        // 2. Fallback: Check if raw JSON is a direct single Layout
        if let Ok(mut layout) = serde_json::from_str::<Layout>(trimmed) {
            sanitize_layout(&mut layout)?;
            return Ok(Self::new_layout(layout, None));
        }

        // 3. Fallback: Check if raw JSON is State or Layouts
        if let Ok(state) = serde_json::from_str::<crate::config::state::State>(trimmed) {
            let mut layouts = state.layout_manager.layouts;
            if layouts.is_empty() {
                return Err(BundleValidationError::EmptyBundle);
            }
            for l in &mut layouts {
                sanitize_layout(l)?;
            }
            let workspace = WorkspaceBundle {
                layouts,
                active_layout: state.layout_manager.active_layout,
                custom_theme: state.custom_theme,
                timezone: Some(state.timezone),
                tickers_table: state.sidebar.tickers_table,
                audio_cfg: Some(state.audio_cfg),
                size_in_quote_ccy: Some(state.size_in_quote_ccy),
                default_kline_config: state.default_kline_config,
            };
            return Ok(Self::new_workspace(workspace, None));
        }

        // 4. Fallback: Check if raw JSON is Layouts
        if let Ok(lm) = serde_json::from_str::<crate::config::state::Layouts>(trimmed) {
            let mut layouts = lm.layouts;
            if layouts.is_empty() {
                return Err(BundleValidationError::EmptyBundle);
            }
            for l in &mut layouts {
                sanitize_layout(l)?;
            }
            let workspace = WorkspaceBundle {
                layouts,
                active_layout: lm.active_layout,
                custom_theme: None,
                timezone: None,
                tickers_table: None,
                audio_cfg: None,
                size_in_quote_ccy: None,
                default_kline_config: None,
            };
            return Ok(Self::new_workspace(workspace, None));
        }

        Err(BundleValidationError::JsonError(
            "Input is not a valid Hawk Terminal configuration or layout".to_string(),
        ))
    }

    pub fn sanitize(&mut self) -> Result<(), BundleValidationError> {
        match &mut self.payload {
            BundlePayload::Layout(layout) => {
                sanitize_layout(layout)?;
            }
            BundlePayload::Workspace(workspace) => {
                if workspace.layouts.is_empty() {
                    return Err(BundleValidationError::EmptyBundle);
                }
                if workspace.layouts.len() > MAX_LAYOUTS_PER_BUNDLE {
                    return Err(BundleValidationError::TooManyLayouts(
                        MAX_LAYOUTS_PER_BUNDLE,
                    ));
                }
                for layout in &mut workspace.layouts {
                    sanitize_layout(layout)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::dashboard::Dashboard;
    use crate::layout::pane::Axis;

    #[test]
    fn test_layout_bundle_roundtrip() {
        let layout = Layout {
            name: "Test Layout".to_string(),
            dashboard: Dashboard::default(),
        };

        let bundle = ConfigBundle::new_layout(
            layout.clone(),
            Some(BundleMetadata {
                name: "Test Layout".to_string(),
                description: Some("Description".to_string()),
                author: Some("Author".to_string()),
            }),
        );

        let json = bundle.to_json_pretty().expect("Failed to serialize");
        let parsed = ConfigBundle::from_json(&json).expect("Failed to parse");

        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.export_type, ExportType::Layout);
        if let BundlePayload::Layout(l) = parsed.payload {
            assert_eq!(l.name, "Test Layout");
        } else {
            panic!("Expected BundlePayload::Layout");
        }
    }

    #[test]
    fn test_legacy_raw_layout_fallback() {
        let raw_layout = Layout {
            name: "Legacy Layout".to_string(),
            dashboard: Dashboard::default(),
        };

        let json = serde_json::to_string(&raw_layout).unwrap();
        let parsed = ConfigBundle::from_json(&json).expect("Failed to parse legacy layout");

        assert_eq!(parsed.export_type, ExportType::Layout);
        if let BundlePayload::Layout(l) = parsed.payload {
            assert_eq!(l.name, "Legacy Layout");
        } else {
            panic!("Expected BundlePayload::Layout");
        }
    }

    #[test]
    fn test_pane_depth_limit() {
        let mut pane = Pane::default();
        for _ in 0..20 {
            pane = Pane::Split {
                axis: Axis::Horizontal,
                ratio: 0.5,
                a: Box::new(pane),
                b: Box::new(Pane::default()),
            };
        }

        let mut layout = Layout {
            name: "Deep Tree".to_string(),
            dashboard: Dashboard {
                pane,
                popout: vec![],
            },
        };

        let result = sanitize_layout(&mut layout);
        assert!(matches!(
            result,
            Err(BundleValidationError::PaneTreeTooDeep(_))
        ));
    }

    #[test]
    fn test_ratio_and_float_sanitization() {
        let mut pane = Pane::Split {
            axis: Axis::Vertical,
            ratio: f32::NAN,
            a: Box::new(Pane::default()),
            b: Box::new(Pane::default()),
        };

        let mut pane_count = 0;
        let _ = validate_and_sanitize_pane(&mut pane, 1, &mut pane_count);
        if let Pane::Split { ratio, .. } = pane {
            assert_eq!(ratio, 0.5);
        } else {
            panic!("Expected Split");
        }
    }
}
