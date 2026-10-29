//! Market state, owned by the hot thread (single writer, no locks).
//!
//! Strategies and the risk engine read it through `&MarketState`. All
//! per-instrument storage is allocated when instruments are loaded, so a
//! tick update does not allocate.

mod ring;

use es_core::{Date, Depth5, Instrument, InstrumentKind, Paise, Tick, Ts, bs};
use ring::Ring;
use serde::Deserialize;
use std::collections::HashMap;

/// Links an underlying name in the instrument master (`NIFTY`) to its index
/// token (`256265`). Config, not code.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UnderlyingSpec {
    pub name: String,
    pub index_token: u32,
}

/// Latest known state of one instrument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quote {
    pub ltp: Paise,
    pub close: Paise,
    pub volume: u64,
    pub oi: u64,
    /// OI at the first tick of the session, for OI change.
    pub oi_open: u64,
    pub depth: Option<Depth5>,
    pub last: Option<Ts>,
}

impl Quote {
    pub fn best_bid(&self) -> Option<Paise> {
        self.depth.and_then(|d| d.best_bid()).map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<Paise> {
        self.depth.and_then(|d| d.best_ask()).map(|l| l.price)
    }

    /// Mid price if both sides exist, otherwise last price.
    pub fn mark(&self) -> Paise {
        match (self.best_bid(), self.best_ask()) {
            (Some(b), Some(a)) => Paise((b.0 + a.0) / 2),
            _ => self.ltp,
        }
    }

    /// Bid-ask spread in basis points of the mid price.
    pub fn spread_bps(&self) -> Option<u32> {
        let (b, a) = (self.best_bid()?, self.best_ask()?);
        let mid = (a.0 + b.0) / 2;
        if mid <= 0 || a < b {
            return None;
        }
        Some(((a.0 - b.0) * 10_000 / mid) as u32)
    }

    pub fn oi_change(&self) -> i64 {
        self.oi as i64 - self.oi_open as i64
    }
}

/// One strike row of an option chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainRow {
    pub strike: Paise,
    pub ce: Option<u32>,
    pub pe: Option<u32>,
}

struct UnderlyingState {
    spec: UnderlyingSpec,
    /// Expiry -> rows sorted by strike.
    chains: Vec<(Date, Vec<ChainRow>)>,
    /// Spot sampled at most once a second.
    spot_hist: Ring,
}

pub struct MarketState {
    instruments: Vec<Instrument>,
    quotes: Vec<Quote>,
    by_token: HashMap<u32, usize>,
    underlyings: Vec<UnderlyingState>,
    index_to_underlying: HashMap<u32, usize>,
    now: Ts,
}

const SPOT_HISTORY_SECS: usize = 1_800;

impl MarketState {
    pub fn new(instruments: Vec<Instrument>, specs: &[UnderlyingSpec]) -> MarketState {
        let by_token = instruments
            .iter()
            .enumerate()
            .map(|(i, ins)| (ins.token, i))
            .collect();
        let mut underlyings = Vec::new();
        let mut index_to_underlying = HashMap::new();
        for (ui, spec) in specs.iter().enumerate() {
            let mut chains: Vec<(Date, Vec<ChainRow>)> = Vec::new();
            for ins in instruments
                .iter()
                .filter(|i| i.is_option() && i.underlying == spec.name)
            {
                let Some(exp) = ins.expiry else { continue };
                let rows = match chains.iter_mut().find(|(d, _)| *d == exp) {
                    Some((_, r)) => r,
                    None => {
                        chains.push((exp, Vec::new()));
                        &mut chains.last_mut().expect("just pushed").1
                    }
                };
                let row = match rows.iter_mut().find(|r| r.strike == ins.strike) {
                    Some(r) => r,
                    None => {
                        rows.push(ChainRow {
                            strike: ins.strike,
                            ce: None,
                            pe: None,
                        });
                        rows.last_mut().expect("just pushed")
                    }
                };
                match ins.kind {
                    InstrumentKind::Ce => row.ce = Some(ins.token),
                    InstrumentKind::Pe => row.pe = Some(ins.token),
                    _ => {}
                }
            }
            for (_, rows) in &mut chains {
                rows.sort_by_key(|r| r.strike);
            }
            chains.sort_by_key(|(d, _)| *d);
            index_to_underlying.insert(spec.index_token, ui);
            underlyings.push(UnderlyingState {
                spec: spec.clone(),
                chains,
                spot_hist: Ring::new(SPOT_HISTORY_SECS),
            });
        }
        let n = instruments.len();
        MarketState {
            instruments,
            quotes: vec![Quote::default(); n],
            by_token,
            underlyings,
            index_to_underlying,
            now: Ts(0),
        }
    }

    /// Applies one tick. Unknown tokens are ignored (returns false).
    pub fn on_tick(&mut self, t: &Tick) -> bool {
        self.now = self.now.max(t.recv);
        let Some(&i) = self.by_token.get(&t.token) else {
            return false;
        };
        let q = &mut self.quotes[i];
        if q.last.is_none() {
            q.oi_open = t.oi;
        }
        q.ltp = t.ltp;
        if t.close.0 > 0 {
            q.close = t.close;
        }
        q.volume = q.volume.max(t.volume);
        if t.oi > 0 {
            q.oi = t.oi;
        }
        if t.depth.is_some() {
            q.depth = t.depth;
        }
        q.last = Some(t.recv);
        if let Some(&ui) = self.index_to_underlying.get(&t.token) {
            self.underlyings[ui].spot_hist.push(t.recv, t.ltp);
        }
        true
    }

    /// Advances engine time without a tick (timers).
    pub fn advance(&mut self, now: Ts) {
        self.now = self.now.max(now);
    }

    pub fn now(&self) -> Ts {
        self.now
    }

    pub fn today(&self) -> Date {
        self.now.ist_date()
    }

    pub fn instrument(&self, token: u32) -> Option<&Instrument> {
        self.by_token.get(&token).map(|&i| &self.instruments[i])
    }

    pub fn instruments(&self) -> &[Instrument] {
        &self.instruments
    }

    pub fn quote(&self, token: u32) -> Option<&Quote> {
        self.by_token.get(&token).map(|&i| &self.quotes[i])
    }

    /// Milliseconds since the instrument's last tick (`None` if never ticked).
    pub fn age_ms(&self, token: u32) -> Option<i64> {
        self.quote(token)?.last.map(|t| self.now.ms_since(t))
    }

    pub fn underlyings(&self) -> impl Iterator<Item = &UnderlyingSpec> {
        self.underlyings.iter().map(|u| &u.spec)
    }

    fn und(&self, name: &str) -> Option<&UnderlyingState> {
        self.underlyings.iter().find(|u| u.spec.name == name)
    }

    pub fn spot(&self, underlying: &str) -> Option<Paise> {
        let u = self.und(underlying)?;
        self.quote(u.spec.index_token)
            .filter(|q| q.last.is_some())
            .map(|q| q.ltp)
    }

    /// Spot change over the last `window_ms`, using the oldest sample inside
    /// the window. `None` until the window has enough history.
    pub fn spot_change(&self, underlying: &str, window_ms: i64) -> Option<Paise> {
        let u = self.und(underlying)?;
        let (_, latest) = u.spot_hist.latest()?;
        let from = self.now.plus_ms(-window_ms);
        let (ts, past) = u.spot_hist.at_or_after(from)?;
        if self.now.ms_since(ts) < window_ms * 9 / 10 {
            return None;
        }
        Some(latest - past)
    }

    /// The nearest expiry on or after today for an underlying.
    pub fn nearest_expiry(&self, underlying: &str) -> Option<Date> {
        let today = self.today();
        self.und(underlying)?
            .chains
            .iter()
            .map(|(d, _)| *d)
            .find(|d| *d >= today)
    }

    pub fn chain(&self, underlying: &str, expiry: Date) -> &[ChainRow] {
        self.und(underlying)
            .and_then(|u| u.chains.iter().find(|(d, _)| *d == expiry))
            .map(|(_, r)| r.as_slice())
            .unwrap_or(&[])
    }

    /// Strike nearest to spot.
    pub fn atm(&self, underlying: &str, expiry: Date) -> Option<ChainRow> {
        let spot = self.spot(underlying)?;
        self.chain(underlying, expiry)
            .iter()
            .min_by_key(|r| (r.strike.0 - spot.0).abs())
            .copied()
    }

    /// Implied volatility of an option from its mark, annualised (0.15 = 15%).
    pub fn iv(&self, token: u32) -> Option<f64> {
        let ins = self.instrument(token)?;
        let spot = self.spot(&ins.underlying)?;
        let expiry = ins.expiry?;
        let close = Ts::at_ist(expiry, 15, 30, 0);
        let secs = (close.secs() - self.now.secs()).max(60) as f64;
        let t = secs / (365.0 * 24.0 * 3600.0);
        let mark = self.quote(token)?.mark();
        bs::implied_vol(
            ins.kind == InstrumentKind::Ce,
            mark.0 as f64 / 100.0,
            spot.0 as f64 / 100.0,
            ins.strike.0 as f64 / 100.0,
            t,
        )
    }
}

#[cfg(test)]
mod tests;
