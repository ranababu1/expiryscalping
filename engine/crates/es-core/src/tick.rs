//! Parsed market ticks.

use crate::{Paise, Ts};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepthLevel {
    pub qty: u32,
    pub price: Paise,
    pub orders: u16,
}

/// Five levels of bids and offers, best first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Depth5 {
    pub bids: [DepthLevel; 5],
    pub asks: [DepthLevel; 5],
}

impl Depth5 {
    pub fn best_bid(&self) -> Option<DepthLevel> {
        self.bids
            .first()
            .copied()
            .filter(|l| l.qty > 0 && l.price.0 > 0)
    }

    pub fn best_ask(&self) -> Option<DepthLevel> {
        self.asks
            .first()
            .copied()
            .filter(|l| l.qty > 0 && l.price.0 > 0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tick {
    pub token: u32,
    pub ltp: Paise,
    pub ltq: u32,
    pub volume: u64,
    pub oi: u64,
    pub close: Paise,
    pub depth: Option<Depth5>,
    /// Exchange timestamp, seconds since the Unix epoch (0 if absent).
    pub exchange_ts: i64,
    /// Local receive time stamped by the feed (or the recording).
    pub recv: Ts,
}
