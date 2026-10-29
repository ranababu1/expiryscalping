//! Momentum breakout on the ATM option of the nearest expiry.
//!
//! Rules (all values are config, defaults below):
//! 1. Wait `warmup_ms` after the first tick of the session.
//! 2. Measure the index move over the last `lookback_ms`.
//! 3. If it rose by at least `trigger_points`, buy the ATM CE. If it fell by
//!    at least `trigger_points`, buy the ATM PE. One position at a time.
//! 4. Exit at `target_bps` above the entry price (target), `stop_bps` below
//!    it (stop loss), or after `time_stop_ms` (time stop), whichever is
//!    first. Exit prices are judged on the best bid, which is what a sell
//!    would get.
//! 5. After an exit, wait `cooldown_ms` before the next entry.
//!
//! The risk engine still decides size, price, timing windows and whether
//! the order goes out at all. This is a baseline to test the plumbing and
//! compare future models against, not a claim of edge.

use crate::Strategy;
use es_core::{Fill, Intent, Paise, ReasonCode, Side, Signal, Tick, Ts};
use es_market::MarketState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MomentumConfig {
    /// Underlying name as in the instrument master, for example `NIFTY`.
    pub underlying: String,
    pub lots: u32,
    pub warmup_ms: i64,
    pub lookback_ms: i64,
    /// Index move, in paise, that triggers an entry.
    pub trigger_points: Paise,
    pub target_bps: i64,
    pub stop_bps: i64,
    pub time_stop_ms: i64,
    pub cooldown_ms: i64,
}

impl Default for MomentumConfig {
    fn default() -> Self {
        MomentumConfig {
            underlying: "NIFTY".into(),
            lots: 1,
            warmup_ms: 5 * 60_000,
            lookback_ms: 60_000,
            trigger_points: Paise::from_rupees(15),
            target_bps: 2_000,
            stop_bps: 1_000,
            time_stop_ms: 3 * 60_000,
            cooldown_ms: 2 * 60_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Flat {
        cooldown_until: Option<Ts>,
    },
    Entering {
        token: u32,
    },
    Long {
        token: u32,
        qty: i64,
        entry: Paise,
        since: Ts,
    },
    Exiting {
        token: u32,
        qty: i64,
    },
}

pub struct MomentumBreakout {
    cfg: MomentumConfig,
    state: State,
    started: Option<Ts>,
    now: Ts,
}

pub const ID: &str = "momentum";

impl MomentumBreakout {
    pub fn new(cfg: MomentumConfig) -> MomentumBreakout {
        MomentumBreakout {
            cfg,
            state: State::Flat {
                cooldown_until: None,
            },
            started: None,
            now: Ts::default(),
        }
    }

    pub fn config(&self) -> &MomentumConfig {
        &self.cfg
    }

    fn signal(&self, token: u32, side: Side, intent: Intent, reason: ReasonCode) -> Signal {
        Signal {
            strategy: ID,
            instrument: token,
            side,
            intent,
            qty_lots: self.cfg.lots,
            limit: None,
            confidence: 0.5,
            reason,
        }
    }

    fn check_entry(&mut self, view: &MarketState, out: &mut Vec<Signal>) {
        let State::Flat { cooldown_until } = self.state else {
            return;
        };
        if cooldown_until.is_some_and(|c| self.now < c) {
            return;
        }
        let Some(start) = self.started else { return };
        if self.now.ms_since(start) < self.cfg.warmup_ms {
            return;
        }
        let und = self.cfg.underlying.as_str();
        let Some(change) = view.spot_change(und, self.cfg.lookback_ms) else {
            return;
        };
        let up = change >= self.cfg.trigger_points;
        let down = change.0 <= -self.cfg.trigger_points.0;
        if !up && !down {
            return;
        }
        let Some(expiry) = view.nearest_expiry(und) else {
            return;
        };
        let Some(row) = view.atm(und, expiry) else {
            return;
        };
        let (token, reason) = if up {
            (row.ce, ReasonCode::MomentumUp)
        } else {
            (row.pe, ReasonCode::MomentumDown)
        };
        let Some(token) = token else { return };
        out.push(self.signal(token, Side::Buy, Intent::Enter, reason));
        self.state = State::Entering { token };
    }

    fn check_exit(&mut self, view: &MarketState, out: &mut Vec<Signal>) {
        let State::Long {
            token,
            qty,
            entry,
            since,
        } = self.state
        else {
            return;
        };
        let reason = if self.now.ms_since(since) >= self.cfg.time_stop_ms {
            Some(ReasonCode::TimeStop)
        } else if let Some(bid) = view.quote(token).and_then(|q| q.best_bid()) {
            let target = entry.0 + entry.0 * self.cfg.target_bps / 10_000;
            let stop = entry.0 - entry.0 * self.cfg.stop_bps / 10_000;
            if bid.0 >= target {
                Some(ReasonCode::Target)
            } else if bid.0 <= stop {
                Some(ReasonCode::StopLoss)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(r) = reason {
            out.push(self.signal(token, Side::Sell, Intent::Exit, r));
            self.state = State::Exiting { token, qty };
        }
    }
}

impl Strategy for MomentumBreakout {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_tick(&mut self, view: &MarketState, tick: &Tick, out: &mut Vec<Signal>) {
        self.now = self.now.max(tick.recv);
        self.started.get_or_insert(tick.recv);
        match self.state {
            State::Flat { .. } => self.check_entry(view, out),
            State::Long { token, .. } if token == tick.token => self.check_exit(view, out),
            _ => {}
        }
    }

    fn on_fill(&mut self, f: &Fill) {
        let signed = f.side.sign() * f.qty as i64;
        self.state = match self.state {
            State::Entering { token } if token == f.token && f.side == Side::Buy => State::Long {
                token,
                qty: signed,
                entry: f.price,
                since: f.ts,
            },
            State::Long {
                token,
                qty,
                entry,
                since,
            } if token == f.token => {
                let q = qty + signed;
                if q <= 0 {
                    State::Flat {
                        cooldown_until: Some(f.ts.plus_ms(self.cfg.cooldown_ms)),
                    }
                } else {
                    // Partial entry fills: average the entry price.
                    let avg = if signed > 0 {
                        Paise((entry.0 * qty + f.price.0 * signed) / q)
                    } else {
                        entry
                    };
                    State::Long {
                        token,
                        qty: q,
                        entry: avg,
                        since,
                    }
                }
            }
            State::Exiting { token, qty } if token == f.token => {
                let q = qty + signed;
                if q <= 0 {
                    State::Flat {
                        cooldown_until: Some(f.ts.plus_ms(self.cfg.cooldown_ms)),
                    }
                } else {
                    State::Exiting { token, qty: q }
                }
            }
            s => s,
        };
    }

    fn on_timer(&mut self, view: &MarketState, now: Ts, out: &mut Vec<Signal>) {
        self.now = self.now.max(now);
        if matches!(self.state, State::Long { .. }) {
            self.check_exit(view, out);
        }
    }

    fn on_rejected(&mut self, instrument: u32, now: Ts) {
        self.state = match self.state {
            State::Entering { token } if token == instrument => State::Flat {
                cooldown_until: Some(now.plus_ms(self.cfg.cooldown_ms)),
            },
            // An exit that did not go out: stay long so the next tick or
            // timer retries it (the kill switch and flatten do not depend on
            // the strategy).
            State::Exiting { token, qty } if token == instrument => State::Long {
                token,
                qty,
                entry: Paise::ZERO,
                since: Ts::default(),
            },
            s => s,
        };
    }

    fn status(&self) -> String {
        match self.state {
            State::Flat {
                cooldown_until: Some(c),
            } if self.now < c => {
                format!("flat, cooling down {}s", c.ms_since(self.now) / 1_000)
            }
            State::Flat { .. } => "flat, watching".into(),
            State::Entering { token } => format!("entering {token}"),
            State::Long {
                token, qty, entry, ..
            } => format!("long {qty} of {token} at {entry}"),
            State::Exiting { token, .. } => format!("exiting {token}"),
        }
    }
}

#[cfg(test)]
mod tests;
