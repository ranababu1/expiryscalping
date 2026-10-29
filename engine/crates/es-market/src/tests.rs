use super::*;
use es_core::{Depth5, DepthLevel};

fn d() -> Date {
    Date::new(2026, 11, 3).unwrap()
}

fn opt(token: u32, strike: i64, kind: InstrumentKind) -> Instrument {
    Instrument {
        token,
        tradingsymbol: format!("NIFTY{strike}{kind:?}"),
        underlying: "NIFTY".into(),
        exchange: "NFO".into(),
        kind,
        expiry: Some(d()),
        strike: Paise::from_rupees(strike),
        tick_size: Paise(5),
        lot_size: 65,
    }
}

fn state() -> MarketState {
    let idx = Instrument {
        token: 256_265,
        tradingsymbol: "NIFTY 50".into(),
        underlying: "NIFTY 50".into(),
        exchange: "NSE".into(),
        kind: InstrumentKind::Index,
        expiry: None,
        strike: Paise::ZERO,
        tick_size: Paise::ZERO,
        lot_size: 0,
    };
    let ins = vec![
        idx,
        opt(1002, 24_000, InstrumentKind::Ce),
        opt(1102, 24_000, InstrumentKind::Pe),
        opt(1202, 24_050, InstrumentKind::Ce),
        opt(1302, 24_050, InstrumentKind::Pe),
    ];
    MarketState::new(
        ins,
        &[UnderlyingSpec {
            name: "NIFTY".into(),
            index_token: 256_265,
        }],
    )
}

fn at(sec: u32) -> Ts {
    Ts::at_ist(d(), 9, 15, 0).plus_ms(sec as i64 * 1000)
}

fn spot_tick(rupees: i64, recv: Ts) -> Tick {
    Tick {
        token: 256_265,
        ltp: Paise::from_rupees(rupees),
        recv,
        ..Tick::default()
    }
}

#[test]
fn chain_and_atm() {
    let mut m = state();
    assert_eq!(m.spot("NIFTY"), None);
    m.on_tick(&spot_tick(24_030, at(0)));
    assert_eq!(m.nearest_expiry("NIFTY"), Some(d()));
    let chain = m.chain("NIFTY", d());
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].ce, Some(1002));
    assert_eq!(chain[0].pe, Some(1102));
    assert_eq!(
        m.atm("NIFTY", d()).unwrap().strike,
        Paise::from_rupees(24_050)
    );
}

#[test]
fn quote_depth_spread_and_oi_change() {
    let mut m = state();
    let mut depth = Depth5::default();
    depth.bids[0] = DepthLevel {
        qty: 65,
        price: Paise(9_950),
        orders: 1,
    };
    depth.asks[0] = DepthLevel {
        qty: 65,
        price: Paise(10_050),
        orders: 1,
    };
    let t = Tick {
        token: 1002,
        ltp: Paise(10_000),
        oi: 1_000,
        depth: Some(depth),
        recv: at(0),
        ..Tick::default()
    };
    assert!(m.on_tick(&t));
    m.on_tick(&Tick {
        oi: 1_300,
        depth: None,
        recv: at(1),
        ..t
    });
    let q = m.quote(1002).unwrap();
    assert_eq!(q.mark(), Paise(10_000));
    assert_eq!(q.spread_bps(), Some(100));
    assert_eq!(q.oi_change(), 300);
    assert!(q.depth.is_some(), "depth kept when a tick has none");
    assert_eq!(m.age_ms(1002), Some(0));
    m.advance(at(3));
    assert_eq!(m.age_ms(1002), Some(2_000));
    assert!(!m.on_tick(&Tick { token: 9, ..t }), "unknown token ignored");
}

#[test]
fn spot_change_needs_full_window() {
    let mut m = state();
    for s in 0..=30 {
        m.on_tick(&spot_tick(24_000 + s as i64, at(s)));
    }
    assert_eq!(m.spot_change("NIFTY", 60_000), None);
    for s in 31..=60 {
        m.on_tick(&spot_tick(24_000 + s as i64, at(s)));
    }
    assert_eq!(m.spot_change("NIFTY", 60_000), Some(Paise::from_rupees(60)));
    assert_eq!(m.spot_change("NIFTY", 10_000), Some(Paise::from_rupees(10)));
}

#[test]
fn iv_is_reasonable() {
    let mut m = state();
    m.on_tick(&spot_tick(24_000, at(0)));
    let mut depth = Depth5::default();
    depth.bids[0] = DepthLevel {
        qty: 65,
        price: Paise(5_000),
        orders: 1,
    };
    depth.asks[0] = DepthLevel {
        qty: 65,
        price: Paise(5_010),
        orders: 1,
    };
    m.on_tick(&Tick {
        token: 1002,
        ltp: Paise(5_005),
        depth: Some(depth),
        recv: at(0),
        ..Tick::default()
    });
    let iv = m.iv(1002).unwrap();
    assert!(iv > 0.05 && iv < 1.0, "{iv}");
}
