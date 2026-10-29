//! Kill switch state.

use es_core::{Date, Ts};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KillCause {
    /// Pressed by Imran (desktop app or CLI).
    Manual,
    /// The daily max loss rule fired. Cannot be re-armed the same day.
    DailyMaxLoss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KillState {
    Armed,
    Triggered { cause: KillCause, at: Ts, day: Date },
}

impl KillState {
    pub fn is_triggered(&self) -> bool {
        matches!(self, KillState::Triggered { .. })
    }
}
