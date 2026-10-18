use iced_core::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeAccent {
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
}

impl ThemeAccent {
    pub const ALL: [Self; 8] = [
        Self::One,
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
        Self::Six,
        Self::Seven,
        Self::Eight,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeColorRole {
    Neutral,
    Secondary,
    Primary,
    Success,
    Danger,
    Warning,
}

impl ThemeColorRole {
    pub const ALL: &'static [Self] = &[
        Self::Neutral,
        Self::Secondary,
        Self::Primary,
        Self::Success,
        Self::Danger,
        Self::Warning,
    ];
}

impl std::str::FromStr for ThemeColorRole {
    type Err = UnknownThemeColor;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|role| role.to_string().eq_ignore_ascii_case(value))
            .ok_or_else(|| UnknownThemeColor::Role(value.to_owned()))
    }
}

impl std::fmt::Display for ThemeColorRole {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Neutral => "Neutral",
            Self::Secondary => "Secondary",
            Self::Primary => "Primary",
            Self::Success => "Success",
            Self::Danger => "Danger",
            Self::Warning => "Warning",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeColorShade {
    Faint,
    Soft,
    #[default]
    Base,
    Strong,
    Vivid,
}

impl ThemeColorShade {
    pub const ALL: &'static [Self] = &[
        Self::Faint,
        Self::Soft,
        Self::Base,
        Self::Strong,
        Self::Vivid,
    ];
}

impl std::fmt::Display for ThemeColorShade {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Faint => "Faint",
            Self::Soft => "Soft",
            Self::Base => "Base",
            Self::Strong => "Strong",
            Self::Vivid => "Vivid",
        };
        formatter.write_str(label)
    }
}

impl std::str::FromStr for ThemeColorShade {
    type Err = UnknownThemeColor;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|shade| shade.to_string().eq_ignore_ascii_case(value))
            .ok_or_else(|| UnknownThemeColor::Shade(value.to_owned()))
    }
}

/// A theme colour name that does not match any known role or shade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownThemeColor {
    Role(String),
    Shade(String),
}

impl std::fmt::Display for UnknownThemeColor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Role(value) => write!(formatter, "unknown theme role '{value}'"),
            Self::Shade(value) => write!(formatter, "unknown theme shade '{value}'"),
        }
    }
}

impl std::error::Error for UnknownThemeColor {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColor {
    pub role: ThemeColorRole,
    pub shade: ThemeColorShade,
    #[serde(default)]
    pub accent: Option<ThemeAccent>,
}

impl Default for ThemeColor {
    fn default() -> Self {
        Self::new(ThemeColorRole::Neutral, ThemeColorShade::Base)
    }
}

impl ThemeColor {
    pub const fn new(role: ThemeColorRole, shade: ThemeColorShade) -> Self {
        Self {
            role,
            shade,
            accent: None,
        }
    }

    pub const fn with_accent(
        role: ThemeColorRole,
        accent: Option<ThemeAccent>,
        shade: ThemeColorShade,
    ) -> Self {
        Self {
            role,
            shade,
            accent,
        }
    }
}

pub fn hex_to_color(hex: &str) -> Option<Color> {
    if hex.len() == 7 || hex.len() == 9 {
        let hash = &hex[0..1];
        let r = u8::from_str_radix(&hex[1..3], 16);
        let g = u8::from_str_radix(&hex[3..5], 16);
        let b = u8::from_str_radix(&hex[5..7], 16);
        let a = (hex.len() == 9)
            .then(|| u8::from_str_radix(&hex[7..9], 16).ok())
            .flatten();

        return match (hash, r, g, b, a) {
            ("#", Ok(r), Ok(g), Ok(b), None) => Some(Color {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: 1.0,
            }),
            ("#", Ok(r), Ok(g), Ok(b), Some(a)) => Some(Color {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: f32::from(a) / 255.0,
            }),
            _ => None,
        };
    }

    None
}

pub fn color_to_hex(color: Color) -> String {
    use std::fmt::Write;

    let mut hex = String::with_capacity(9);
    let [r, g, b, a] = color.into_rgba8();

    let _ = write!(&mut hex, "#{r:02X}{g:02X}{b:02X}");
    if a < u8::MAX {
        let _ = write!(&mut hex, "{a:02X}");
    }
    hex
}
