#![allow(clippy::field_reassign_with_default)]

//! Risk rule tests: one or more per rule, with boundaries (exactly at the
//! limit and one paisa or one unit over), exits always pass, and the kill
//! switch.

mod entries;
mod exits_and_kill;
mod limits_changes;

use crate::*;
use es_core::{Date, Intent, Paise, ReasonCode, Side, Signal, Ts};

pub(crate) const TOKEN: u32 = 1002;
pub(crate) const LOT: u32 = 65;

pub(crate) fn day() -> Date {
    Date::new(2026, 11, 3).unwrap()
}

/// 10:00 IST on the test day, plus `ms`.
pub(crate) fn t(ms: i64) -> Ts {
    Ts::at_ist(day(), 10, 0, 0).plus_ms(ms)
}

pub(crate) fn at(h: u32, m: u32) -> Ts {
    Ts::at_ist(day(), h, m, 0)
}

pub(crate) fn engine() -> RiskEngine {
    RiskEngine::new(RiskLimits::default(), MarketHours::default()).unwrap()
}

/// Bid 99.95, ask 100.00 (5 bps spread), fresh.
pub(crate) fn inst() -> InstrumentView {
    InstrumentView {
        token: TOKEN,
        lot_size: LOT,
        tick_size: Paise(5),
        expiry: Some(day()),
        best_bid: Some(Paise(9_995)),
        best_ask: Some(Paise(10_000)),
        ltp: Paise(10_000),
        age_ms: Some(100),
    }
}

pub(crate) fn enter(lots: u32) -> Signal {
    Signal {
        strategy: "test",
        instrument: TOKEN,
        side: Side::Buy,
        intent: Intent::Enter,
        qty_lots: lots,
        limit: None,
        confidence: 1.0,
        reason: ReasonCode::MomentumUp,
    }
}

pub(crate) fn exit() -> Signal {
    Signal {
        side: Side::Sell,
        intent: Intent::Exit,
        reason: ReasonCode::StopLoss,
        ..enter(0)
    }
}

pub(crate) struct Book {
    pub realised: Paise,
    pub unrealised: Paise,
    pub positions: Vec<PositionView>,
    pub pending: Vec<u32>,
    pub funds: Paise,
    pub feed_stale: bool,
}

impl Book {
    pub fn flat() -> Book {
        Book {
            realised: Paise::ZERO,
            unrealised: Paise::ZERO,
            positions: vec![],
            pending: vec![],
            funds: Paise::from_rupees(100_000),
            feed_stale: false,
        }
    }

    pub fn long(token: u32, qty: i64) -> Book {
        let mut b = Book::flat();
        b.positions.push(PositionView {
            token,
            qty,
            pending_exit_qty: 0,
        });
        b
    }

    pub fn view(&self) -> PortfolioView<'_> {
        PortfolioView {
            realised: self.realised,
            unrealised: self.unrealised,
            positions: &self.positions,
            pending_entry_tokens: &self.pending,
            available_funds: self.funds,
            feed_stale: self.feed_stale,
        }
    }
}

pub(crate) fn rejected(d: &RiskDecision) -> Option<RejectReason> {
    match d {
        RiskDecision::Reject(r) => Some(*r),
        _ => None,
    }
}
