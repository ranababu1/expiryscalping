use super::*;
use es_core::{Date, Depth5, DepthLevel, Instrument, InstrumentKind, OrderId};
use es_market::UnderlyingSpec;

const IDX: u32 = 256_265;
const CE: u32 = 1001;
const PE: u32 = 1002;

fn day() -> Date {
    Date::new(2026, 11, 3).unwrap()
}

fn at(ms: i64) -> Ts {
    Ts::at_ist(day(), 9, 20, 0).plus_ms(ms)
}

fn ins(token: u32, kind: InstrumentKind) -> Instrument {
    let index = kind == InstrumentKind::Index;
    Instrument {
        token,
        tradingsymbol: format!("T{token}"),
        underlying: "NIFTY".into(),
        exchange: if index { "NSE" } else { "NFO" }.into(),
        kind,
        expiry: (!index).then(day),
        strike: if index {
            Paise::ZERO
        } else {
            Paise::from_rupees(24_000)
        },
        tick_size: Paise(5),
        lot_size: 65,
    }
}

fn market() -> MarketState {
    MarketState::new(
        vec![
            ins(IDX, InstrumentKind::Index),
            ins(CE, InstrumentKind::Ce),
            ins(PE, InstrumentKind::Pe),
        ],
        &[UnderlyingSpec {
            name: "NIFTY".into(),
            index_token: IDX,
        }],
    )
}

fn cfg() -> MomentumConfig {
    MomentumConfig {
        warmup_ms: 0,
        ..MomentumConfig::default()
    }
}

fn index_tick(ms: i64, rupees: i64) -> Tick {
    Tick {
        token: IDX,
        ltp: Paise::from_rupees(rupees),
        recv: at(ms),
        ..Tick::default()
    }
}

fn opt_tick(token: u32, ms: i64, bid: i64, ask: i64) -> Tick {
    let mut d = Depth5::default();
    d.bids[0] = DepthLevel {
        qty: 650,
        price: Paise(bid),
        orders: 3,
    };
    d.asks[0] = DepthLevel {
        qty: 650,
        price: Paise(ask),
        orders: 3,
    };
    Tick {
        token,
        ltp: Paise(ask),
        depth: Some(d),
        recv: at(ms),
        ..Tick::default()
    }
}

fn fill(token: u32, side: Side, price: i64, ms: i64) -> Fill {
    Fill {
        order: OrderId(1),
        token,
        side,
        qty: 65,
        price: Paise(price),
        ts: at(ms),
        strategy: ID.into(),
    }
}

/// Feeds one index tick per second from 24,000 moving `per_sec` rupees.
fn run_index(
    s: &mut MomentumBreakout,
    m: &mut MarketState,
    secs: i64,
    per_sec: i64,
) -> Vec<Signal> {
    let mut out = Vec::new();
    for i in 0..=secs {
        let t = index_tick(i * 1_000, 24_000 + i * per_sec);
        m.on_tick(&t);
        s.on_tick(m, &t, &mut out);
    }
    out
}

#[test]
fn quiet_market_gives_no_signal() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    assert!(run_index(&mut s, &mut m, 90, 0).is_empty());
    assert_eq!(s.status(), "flat, watching");
}

#[test]
fn up_move_buys_atm_call_once() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    let out = run_index(&mut s, &mut m, 90, 1);
    assert_eq!(out.len(), 1);
    let sig = &out[0];
    assert_eq!(
        (sig.instrument, sig.side, sig.intent, sig.reason),
        (CE, Side::Buy, Intent::Enter, ReasonCode::MomentumUp)
    );
    assert_eq!(sig.qty_lots, 1);
}

#[test]
fn down_move_buys_atm_put() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    let out = run_index(&mut s, &mut m, 90, -1);
    assert_eq!(out[0].instrument, PE);
    assert_eq!(out[0].reason, ReasonCode::MomentumDown);
}

#[test]
fn warmup_delays_entries() {
    let c = MomentumConfig {
        warmup_ms: 120_000,
        ..MomentumConfig::default()
    };
    let (mut s, mut m) = (MomentumBreakout::new(c), market());
    assert!(run_index(&mut s, &mut m, 110, 1).is_empty());
}

#[test]
fn target_stop_and_time_stop() {
    for (bid, ms, expect) in [
        (12_000, 1_000, ReasonCode::Target),  // +20% exactly
        (9_000, 1_000, ReasonCode::StopLoss), // -10% exactly
        (10_100, 180_000, ReasonCode::TimeStop),
    ] {
        let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
        run_index(&mut s, &mut m, 90, 1);
        s.on_fill(&fill(CE, Side::Buy, 10_000, 90_000));
        let mut out = Vec::new();
        // Not yet: inside the band.
        let t = opt_tick(CE, 90_500, 10_500, 10_510);
        m.on_tick(&t);
        s.on_tick(&m, &t, &mut out);
        assert!(out.is_empty());
        let t = opt_tick(CE, 90_000 + ms, bid, bid + 10);
        m.on_tick(&t);
        s.on_tick(&m, &t, &mut out);
        assert_eq!(out.len(), 1, "{expect:?}");
        assert_eq!(
            (out[0].intent, out[0].side, out[0].reason),
            (Intent::Exit, Side::Sell, expect)
        );
    }
}

#[test]
fn time_stop_fires_from_timer_without_ticks() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    run_index(&mut s, &mut m, 90, 1);
    s.on_fill(&fill(CE, Side::Buy, 10_000, 90_000));
    let mut out = Vec::new();
    s.on_timer(&m, at(90_000 + 179_999), &mut out);
    assert!(out.is_empty());
    s.on_timer(&m, at(90_000 + 180_000), &mut out);
    assert_eq!(out[0].reason, ReasonCode::TimeStop);
}

#[test]
fn cooldown_after_exit_then_rejection_resets() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    run_index(&mut s, &mut m, 90, 1);
    s.on_fill(&fill(CE, Side::Buy, 10_000, 90_000));
    let mut out = Vec::new();
    s.on_timer(&m, at(270_000), &mut out);
    s.on_fill(&fill(CE, Side::Sell, 10_000, 270_000));
    assert!(s.status().starts_with("flat, cooling down"));
    // Strong move during cooldown: no entry.
    let mut out = Vec::new();
    for i in 271..300 {
        let t = index_tick(i * 1_000, 24_200 + i);
        m.on_tick(&t);
        s.on_tick(&m, &t, &mut out);
    }
    assert!(out.is_empty());
    // After cooldown an entry comes; risk rejects it; the strategy resets.
    for i in 330..400 {
        let t = index_tick(i * 1_000, 24_300 + i * 2);
        m.on_tick(&t);
        s.on_tick(&m, &t, &mut out);
    }
    assert_eq!(out.len(), 1);
    s.on_rejected(out[0].instrument, at(400_000));
    assert!(s.status().starts_with("flat"));
}

#[test]
fn rejected_exit_is_retried() {
    let (mut s, mut m) = (MomentumBreakout::new(cfg()), market());
    run_index(&mut s, &mut m, 90, 1);
    s.on_fill(&fill(CE, Side::Buy, 10_000, 90_000));
    let mut out = Vec::new();
    s.on_timer(&m, at(270_000), &mut out);
    assert_eq!(out.len(), 1);
    s.on_rejected(CE, at(270_001));
    s.on_timer(&m, at(270_100), &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].intent, Intent::Exit);
}
