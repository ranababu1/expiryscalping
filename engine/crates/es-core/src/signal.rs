//! Signals emitted by strategies (and by manual actions from a client).
//! A signal is a wish. Only the risk engine can turn it into an order.

use crate::{Paise, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    /// Open or add to a position.
    Enter,
    /// Close the whole position in the instrument.
    Exit,
    /// Reduce the position to this many lots.
    ReduceTo(u32),
}

impl Intent {
    pub fn is_exit(self) -> bool {
        !matches!(self, Intent::Enter)
    }
}

/// Compact reason logged with every order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReasonCode {
    MomentumUp,
    MomentumDown,
    Target,
    StopLoss,
    TimeStop,
    Manual,
    KillSwitch,
    Flatten,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Signal {
    pub strategy: &'static str,
    pub instrument: u32,
    pub side: Side,
    pub intent: Intent,
    pub qty_lots: u32,
    pub limit: Option<Paise>,
    pub confidence: f32,
    pub reason: ReasonCode,
}
