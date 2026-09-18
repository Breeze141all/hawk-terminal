use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayState {
    pub active: bool,
    pub cutoff_time: u64,
    pub is_playing: bool,
    pub speed_ms: u64,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            active: false,
            cutoff_time: 0,
            is_playing: false,
            speed_ms: 1000,
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
        }
    }
}
