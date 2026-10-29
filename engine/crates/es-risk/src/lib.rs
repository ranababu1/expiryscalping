//! The risk engine. Every order, from a strategy or from a manual action,
//! passes through [`RiskEngine::check`]. It cannot be disabled.
//!
//! Enforcement by visibility: [`OrderRequest`] and [`ApprovedModify`] have
//! private fields and no public constructor, so code outside this crate can
//! only obtain them from the risk engine. The broker and order manager accept
//! nothing else.
//!
//! Rules (plan section 9): daily max loss (triggers the kill switch),
//! per-trade max premium and lots, max open positions, max trades per day,
//! order rate cap, entry window and flatten time, expiry day only, stale data
//! guard, spread guard, consecutive loss pause, margin check, kill switch.
//! Exits are always allowed.

mod engine;
mod kill;
mod limits;
mod rate;
mod types;

pub use engine::{RiskAction, RiskEngine, RiskUsage};
pub use kill::{KillCause, KillState};
pub use limits::{HhMm, LimitsError, MarketHours, RiskLimits};
pub use types::{
    ApprovedModify, InstrumentView, OpenOrderView, OrderRequest, PortfolioView, PositionView,
    RejectReason, RiskDecision,
};

#[cfg(test)]
mod tests;
