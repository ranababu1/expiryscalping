//! Synthetic expiry day.
//!
//! The index follows a random walk with trend bursts (so a momentum strategy
//! has something to find and something to lose on). Options on a fixed grid
//! of strikes around the opening ATM are priced with Black-Scholes and a mild
//! smile, as time runs out towards 15:30. Each step emits one Kite binary
//! frame with an index packet per underlying and a full mode packet per
//! option, so the engine parses simulated data exactly like live data.

use crate::rng::Rng;
use es_core::{Date, DepthLevel, Instrument, InstrumentKind, Paise, Ts, bs};
use es_feed::{RawRecord, RecordKind};
use es_kite::instruments::to_csv;
use es_kite::ticker::{FrameBuilder, FullPacket, IndexPacket};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SimUnderlying {
    /// Underlying name used in the instrument master, for example `NIFTY`.
    pub name: String,
    pub index_token: u32,
    pub index_symbol: String,
    /// Options exchange: `NFO` or `BFO`.
    pub exchange: String,
    pub spot_rupees: i64,
    pub strike_step_rupees: i64,
    /// Lot size used by the simulator's instrument master.
    pub lot_size: u32,
    pub tick_size_paise: i64,
    pub strikes_each_side: u32,
    /// Annualised volatility in percent, for example 16.0.
    pub vol_pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimConfig {
    pub seed: u64,
    /// Virtual time between frames, in milliseconds.
    pub step_ms: i64,
    /// Session start and end, IST, as `HH:MM`.
    pub start: String,
    pub end: String,
    pub underlyings: Vec<SimUnderlying>,
}

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("bad time {0:?}, expected HH:MM")]
    BadTime(String),
    #[error("bad sim config: {0}")]
    Invalid(String),
}

pub fn parse_hhmm(s: &str) -> Result<(u32, u32), SimError> {
    let bad = || SimError::BadTime(s.to_string());
    let (h, m) = s.trim().split_once(':').ok_or_else(bad)?;
    let h: u32 = h.parse().map_err(|_| bad())?;
    let m: u32 = m.parse().map_err(|_| bad())?;
    if h > 23 || m > 59 {
        return Err(bad());
    }
    Ok((h, m))
}

struct SimOption {
    token: u32,
    strike: f64,
    is_call: bool,
    oi: u32,
    volume: u32,
    open: Paise,
    high: Paise,
    low: Paise,
}

struct SimIndex {
    cfg: SimUnderlying,
    spot: f64,
    open: f64,
    high: f64,
    low: f64,
    prev_close: f64,
    drift: f64,
    regime_left: u32,
    options: Vec<SimOption>,
}

pub struct Simulator {
    date: Date,
    now: Ts,
    end: Ts,
    step_ms: i64,
    rng: Rng,
    indices: Vec<SimIndex>,
    instruments: Vec<Instrument>,
    sent_master: bool,
}

/// Kite weekly option month code: 1-9, then O, N, D.
fn month_code(m: u8) -> char {
    match m {
        10 => 'O',
        11 => 'N',
        12 => 'D',
        _ => (b'0' + m) as char,
    }
}

fn round_to_tick(v: f64, tick: i64) -> Paise {
    let paise = (v * 100.0 / tick as f64).round() as i64 * tick;
    Paise(paise.max(tick))
}

impl Simulator {
    pub fn new(cfg: &SimConfig, date: Date) -> Result<Simulator, SimError> {
        if cfg.step_ms <= 0 {
            return Err(SimError::Invalid("step_ms must be positive".into()));
        }
        if cfg.underlyings.is_empty() {
            return Err(SimError::Invalid("no underlyings".into()));
        }
        let (sh, sm) = parse_hhmm(&cfg.start)?;
        let (eh, em) = parse_hhmm(&cfg.end)?;
        let start = Ts::at_ist(date, sh, sm, 0);
        let end = Ts::at_ist(date, eh, em, 0);
        if end <= start {
            return Err(SimError::Invalid("end must be after start".into()));
        }
        let mut rng = Rng::new(cfg.seed);
        let mut instruments = Vec::new();
        let mut indices = Vec::new();
        for (ui, u) in cfg.underlyings.iter().enumerate() {
            if u.strike_step_rupees <= 0 || u.lot_size == 0 || u.tick_size_paise <= 0 {
                return Err(SimError::Invalid(format!("bad numbers for {}", u.name)));
            }
            let seg = if u.exchange == "BFO" { 5 } else { 2 };
            instruments.push(Instrument {
                token: u.index_token,
                tradingsymbol: u.index_symbol.clone(),
                underlying: u.index_symbol.clone(),
                exchange: if seg == 5 { "BSE".into() } else { "NSE".into() },
                kind: InstrumentKind::Index,
                expiry: None,
                strike: Paise::ZERO,
                tick_size: Paise::ZERO,
                lot_size: 0,
            });
            let spot = u.spot_rupees as f64;
            let atm = (u.spot_rupees + u.strike_step_rupees / 2) / u.strike_step_rupees
                * u.strike_step_rupees;
            let n = u.strikes_each_side as i64;
            let mut options = Vec::new();
            let mut idx = 0u32;
            for k in -n..=n {
                let strike = atm + k * u.strike_step_rupees;
                for is_call in [true, false] {
                    let token = ((100_000 * (ui as u32 + 1) + idx) << 8) | seg;
                    idx += 1;
                    let suffix = if is_call { "CE" } else { "PE" };
                    let sym = format!(
                        "{}{:02}{}{:02}{}{}",
                        u.name,
                        date.year % 100,
                        month_code(date.month),
                        date.day,
                        strike,
                        suffix
                    );
                    instruments.push(Instrument {
                        token,
                        tradingsymbol: sym,
                        underlying: u.name.clone(),
                        exchange: u.exchange.clone(),
                        kind: if is_call {
                            InstrumentKind::Ce
                        } else {
                            InstrumentKind::Pe
                        },
                        expiry: Some(date),
                        strike: Paise::from_rupees(strike),
                        tick_size: Paise(u.tick_size_paise),
                        lot_size: u.lot_size,
                    });
                    let near = (n - k.abs() + 1) as u64;
                    options.push(SimOption {
                        token,
                        strike: strike as f64,
                        is_call,
                        oi: (rng.range(20, 60) * near * 2_000) as u32 / u.lot_size * u.lot_size,
                        volume: 0,
                        open: Paise::ZERO,
                        high: Paise::ZERO,
                        low: Paise::ZERO,
                    });
                }
            }
            let prev_close = spot * (1.0 + 0.004 * (rng.unit() - 0.5));
            indices.push(SimIndex {
                cfg: u.clone(),
                spot,
                open: spot,
                high: spot,
                low: spot,
                prev_close,
                drift: 0.0,
                regime_left: 0,
                options,
            });
        }
        Ok(Simulator {
            date,
            now: start,
            end,
            step_ms: cfg.step_ms,
            rng,
            indices,
            instruments,
            sent_master: false,
        })
    }

    pub fn instruments(&self) -> &[Instrument] {
        &self.instruments
    }

    pub fn date(&self) -> Date {
        self.date
    }

    fn step(&mut self) -> Vec<u8> {
        let step_secs = self.step_ms as f64 / 1000.0;
        let close = Ts::at_ist(self.date, 15, 30, 0);
        let secs_left = (close.secs() - self.now.secs()).max(120) as f64;
        let t_years = secs_left / (365.0 * 24.0 * 3600.0);
        let exch_ts = self.now.secs() as i32;
        let mut fb = FrameBuilder::new();
        for ix in &mut self.indices {
            let vol = ix.cfg.vol_pct / 100.0;
            // Trading-time volatility per step (about 6.25 trading hours a day).
            let sigma = vol * (step_secs / (252.0 * 6.25 * 3600.0)).sqrt();
            if ix.regime_left == 0 {
                let r = self.rng.unit();
                // Trend bursts move roughly 0.2% to 0.4% over a few minutes.
                let burst = sigma * (0.25 + 0.25 * self.rng.unit());
                ix.drift = if r < 0.25 {
                    burst
                } else if r < 0.5 {
                    -burst
                } else {
                    0.0
                };
                ix.regime_left = (self.rng.range(120, 900) as f64 / step_secs).max(1.0) as u32;
            }
            ix.regime_left -= 1;
            ix.spot *= 1.0 + ix.drift + sigma * self.rng.normal();
            ix.high = ix.high.max(ix.spot);
            ix.low = ix.low.min(ix.spot);
            let p = |v: f64| Paise((v * 100.0).round() as i64);
            fb.push(
                &IndexPacket {
                    token: ix.cfg.index_token,
                    ltp: p(ix.spot),
                    high: p(ix.high),
                    low: p(ix.low),
                    open: p(ix.open),
                    close: p(ix.prev_close),
                    change: p(ix.spot - ix.prev_close),
                    exchange_ts: exch_ts,
                }
                .encode(),
            );
            let tick = ix.cfg.tick_size_paise;
            let lot = ix.cfg.lot_size;
            for o in &mut ix.options {
                let smile = 0.5 * (o.strike / ix.spot).ln().abs();
                let fair = bs::price(o.is_call, ix.spot, o.strike, t_years, vol + smile);
                let mid = round_to_tick(fair, tick);
                let spread_ticks = 1 + (mid.0 as f64 * 0.002 / tick as f64) as i64;
                let best_bid = Paise((mid.0 - (spread_ticks / 2) * tick).max(tick));
                let best_ask = Paise(best_bid.0 + spread_ticks * tick);
                let mut depth = es_core::Depth5::default();
                for i in 0..5 {
                    let qty = |rng: &mut Rng| rng.range(1, 40) as u32 * lot;
                    depth.bids[i] = DepthLevel {
                        qty: qty(&mut self.rng),
                        price: Paise((best_bid.0 - i as i64 * tick).max(tick)),
                        orders: self.rng.range(1, 20) as u16,
                    };
                    depth.asks[i] = DepthLevel {
                        qty: qty(&mut self.rng),
                        price: Paise(best_ask.0 + i as i64 * tick),
                        orders: self.rng.range(1, 20) as u16,
                    };
                }
                let ltp = if self.rng.unit() < 0.5 {
                    best_bid
                } else {
                    best_ask
                };
                let ltq = self.rng.range(1, 10) as u32 * lot;
                o.volume = o.volume.saturating_add(ltq * self.rng.range(1, 5) as u32);
                let doi = self.rng.range(0, 40) as i64 - 20;
                o.oi = (o.oi as i64 + doi * lot as i64).max(lot as i64) as u32;
                if o.open == Paise::ZERO {
                    o.open = ltp;
                    o.high = ltp;
                    o.low = ltp;
                }
                o.high = o.high.max(ltp);
                o.low = o.low.min(ltp);
                fb.push(
                    &FullPacket {
                        token: o.token,
                        ltp,
                        ltq,
                        avg_price: mid,
                        volume: o.volume,
                        buy_qty: depth.bids.iter().map(|l| l.qty).sum(),
                        sell_qty: depth.asks.iter().map(|l| l.qty).sum(),
                        open: o.open,
                        high: o.high,
                        low: o.low,
                        close: o.open,
                        last_trade_ts: exch_ts,
                        oi: o.oi,
                        oi_high: o.oi,
                        oi_low: o.oi,
                        exchange_ts: exch_ts,
                        depth,
                    }
                    .encode(),
                );
            }
        }
        fb.finish()
    }
}

impl Iterator for Simulator {
    type Item = RawRecord;

    fn next(&mut self) -> Option<RawRecord> {
        if !self.sent_master {
            self.sent_master = true;
            return Some(RawRecord {
                kind: RecordKind::Instruments,
                recv: Ts(self.now.0 - 1_000_000_000),
                bytes: to_csv(&self.instruments).into_bytes(),
            });
        }
        if self.now > self.end {
            return None;
        }
        let bytes = self.step();
        let rec = RawRecord {
            kind: RecordKind::Binary,
            recv: self.now,
            bytes,
        };
        self.now = self.now.plus_ms(self.step_ms);
        Some(rec)
    }
}

/// A small config for tests and examples: Nifty and Sensex, 3 strikes each side.
pub fn example_config() -> SimConfig {
    SimConfig {
        seed: 7,
        step_ms: 1_000,
        start: "09:15".into(),
        end: "15:30".into(),
        underlyings: vec![
            SimUnderlying {
                name: "NIFTY".into(),
                index_token: 256_265,
                index_symbol: "NIFTY 50".into(),
                exchange: "NFO".into(),
                spot_rupees: 24_000,
                strike_step_rupees: 50,
                lot_size: 65,
                tick_size_paise: 5,
                strikes_each_side: 3,
                vol_pct: 16.0,
            },
            SimUnderlying {
                name: "SENSEX".into(),
                index_token: 265,
                index_symbol: "SENSEX".into(),
                exchange: "BFO".into(),
                spot_rupees: 80_000,
                strike_step_rupees: 100,
                lot_size: 20,
                tick_size_paise: 5,
                strikes_each_side: 3,
                vol_pct: 16.0,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use es_kite::instruments::parse_csv;
    use es_kite::ticker::{FrameKind, Parser};

    fn date() -> Date {
        Date::new(2026, 11, 3).unwrap()
    }

    #[test]
    fn emits_master_then_parseable_frames() {
        let mut sim = Simulator::new(&example_config(), date()).unwrap();
        let master = sim.next().unwrap();
        assert_eq!(master.kind, RecordKind::Instruments);
        let inst = parse_csv(std::str::from_utf8(&master.bytes).unwrap()).unwrap();
        // 2 indices + 2 underlyings * 7 strikes * 2 sides
        assert_eq!(inst.len(), 2 + 2 * 7 * 2);
        assert!(
            inst.iter()
                .filter(|i| i.is_option())
                .all(|i| i.expiry == Some(date()))
        );

        let mut out = Vec::new();
        for rec in sim.by_ref().take(50) {
            out.clear();
            let kind = Parser.parse_frame(&rec.bytes, rec.recv, &mut out).unwrap();
            assert_eq!(
                kind,
                FrameKind::Data {
                    ticks: 30,
                    skipped: 0
                }
            );
            for t in out.iter().filter(|t| t.depth.is_some()) {
                let d = t.depth.unwrap();
                assert!(d.best_ask().unwrap().price > d.best_bid().unwrap().price);
                assert!(t.ltp.0 > 0);
            }
        }
    }

    #[test]
    fn same_seed_same_day() {
        let a: Vec<_> = Simulator::new(&example_config(), date())
            .unwrap()
            .take(200)
            .collect();
        let b: Vec<_> = Simulator::new(&example_config(), date())
            .unwrap()
            .take(200)
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn full_day_frame_count_and_time_range() {
        let mut cfg = example_config();
        cfg.step_ms = 60_000;
        let recs: Vec<_> = Simulator::new(&cfg, date()).unwrap().collect();
        // master + one frame per minute from 09:15 to 15:30 inclusive
        assert_eq!(recs.len(), 1 + 376);
        assert_eq!(recs[1].recv.ist_minute_of_day(), 9 * 60 + 15);
        assert_eq!(recs.last().unwrap().recv.ist_minute_of_day(), 15 * 60 + 30);
    }

    #[test]
    fn rejects_bad_config() {
        let mut cfg = example_config();
        cfg.start = "25:00".into();
        assert!(Simulator::new(&cfg, date()).is_err());
        let mut cfg = example_config();
        cfg.step_ms = 0;
        assert!(Simulator::new(&cfg, date()).is_err());
    }
}
