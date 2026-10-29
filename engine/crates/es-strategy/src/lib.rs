//! Signal layer. Strategies read market state and emit [`Signal`]s. They
//! never call the broker: every signal goes through the risk engine.
//!
//! Time is engine time ([`Ts`], from tick receive times), so a replay gives
//! the same signals every run.

pub mod momentum;

use es_core::{Fill, Signal, Tick, Ts};
use es_market::MarketState;

pub use momentum::{MomentumBreakout, MomentumConfig};

pub trait Strategy: Send + Sync {
    fn id(&self) -> &'static str;
    fn on_tick(&mut self, view: &MarketState, tick: &Tick, out: &mut Vec<Signal>);
    fn on_fill(&mut self, fill: &Fill);
    fn on_timer(&mut self, view: &MarketState, now: Ts, out: &mut Vec<Signal>);
    /// A signal of ours was rejected by risk, or its order ended without a
    /// fill, so the strategy can reset.
    fn on_rejected(&mut self, instrument: u32, now: Ts);
    /// Human-readable state for the app ("flat", "long NIFTY 24000 CE").
    fn status(&self) -> String;
}
