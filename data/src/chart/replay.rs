use serde::{Deserialize, Serialize};

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayState {
    pub active: bool,
    pub cutoff_time: u64,
    pub is_playing: bool,
    pub speed_ms: u64,
    #[serde(default = "default_true")]
    pub auto_scroll: bool,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            active: false,
            cutoff_time: 0,
            is_playing: false,
            speed_ms: 1000,
            auto_scroll: true,
        }
    }
}

impl ReplayState {
    pub fn new(cutoff_time: u64) -> Self {
        Self {
            active: true,
            cutoff_time,
            is_playing: false,
            speed_ms: 1000,
            auto_scroll: true,
        }
    }
}
