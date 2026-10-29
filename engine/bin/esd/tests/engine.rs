//! End-to-end engine tests on the simulator: determinism of replay, the kill
//! switch, flatten time, the daily loss limit and manual commands.

use es_api::{ClientMsg, ErrorCode, Mode};
use es_archiver::{RawLogReader, RawLogWriter};
use es_core::{Date, Paise};
use es_feed::RawRecord;
use es_risk::{HhMm, RiskLimits};
use esd::config::EngineConfig;
use esd::dto::limits_to_dto;
use esd::{Engine, EngineOptions, Journal};
use std::path::Path;

const DAY: Date = Date {
    year: 2026,
    month: 10,
    day: 29,
};

fn config(end: &str) -> EngineConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/engine.toml");
    let mut cfg = EngineConfig::load(&path).expect("shipped config");
    cfg.sim.end = end.into();
    cfg
}

fn records(cfg: &EngineConfig) -> Vec<RawRecord> {
    es_replay::Simulator::new(&cfg.sim, DAY)
        .expect("sim")
        .collect()
}

fn engine(cfg: EngineConfig, limits: RiskLimits) -> Engine {
    let opts = EngineOptions {
        source: "test".into(),
        version: "test".into(),
        data_dir: None,
    };
    Engine::new(cfg, limits, opts, Journal::in_memory(), None).expect("engine")
}

async fn feed(e: &mut Engine, recs: &[RawRecord]) {
    for r in recs {
        e.on_record(r.clone()).await;
    }
}

/// Feeds records until `cond` holds. Returns how many were used.
async fn feed_until(e: &mut Engine, recs: &[RawRecord], cond: impl Fn(&Engine) -> bool) -> usize {
    for (i, r) in recs.iter().enumerate() {
        e.on_record(r.clone()).await;
        if cond(e) {
            return i + 1;
        }
    }
    panic!("condition never held");
}

fn open_positions(e: &Engine) -> usize {
    e.positions_dto()
        .positions
        .iter()
        .filter(|p| p.qty != 0)
        .count()
}

fn fills(e: &Engine) -> usize {
    e.journal()
        .lines()
        .iter()
        .filter(|l| l.contains("\"kind\":\"fill\""))
        .count()
}

#[tokio::test]
async fn replay_of_recorded_sim_gives_identical_journal() {
    let cfg = config("11:30");
    let recs = records(&cfg);

    // Record the sim to a raw log, as `esd sim --record` does.
    let mut buf = Vec::new();
    {
        let mut w = RawLogWriter::new(&mut buf).unwrap();
        for r in &recs {
            w.write(r).unwrap();
        }
        w.flush().unwrap();
    }

    let mut live = engine(cfg.clone(), RiskLimits::default());
    feed(&mut live, &recs).await;

    let replayed: Vec<RawRecord> = RawLogReader::new(buf.as_slice())
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(replayed.len(), recs.len());
    let mut again = engine(cfg, RiskLimits::default());
    feed(&mut again, &replayed).await;

    assert!(fills(&live) > 0, "the strategy should trade in two hours");
    assert_eq!(live.journal().lines(), again.journal().lines());
}

#[tokio::test]
async fn kill_switch_flattens_disables_and_blocks_entries() {
    let cfg = config("12:30");
    let recs = records(&cfg);
    let mut e = engine(cfg, RiskLimits::default());
    let used = feed_until(&mut e, &recs, |e| open_positions(e) > 0).await;

    let ack = e.execute("test", ClientMsg::Kill { req_id: 1 }).await;
    assert!(ack.is_ok());
    assert!(e.status_dto().kill.triggered);
    assert!(e.strategies_dto().strategies.iter().all(|s| !s.enabled));

    // A few seconds of market later the book is flat.
    feed(&mut e, &recs[used..used + 30]).await;
    assert_eq!(open_positions(&e), 0, "kill switch must flatten");

    // Entries are refused while the kill switch is on.
    let token = e.chain_dto().rows[0].ce.as_ref().unwrap().token;
    let r = e
        .execute(
            "test",
            ClientMsg::Place {
                req_id: 2,
                token,
                lots: 1,
                limit: None,
            },
        )
        .await;
    assert!(matches!(r, Err((ErrorCode::RiskRejected, _))));
    let r = e
        .execute(
            "test",
            ClientMsg::SetStrategy {
                req_id: 3,
                id: "momentum".into(),
                enabled: true,
            },
        )
        .await;
    assert!(matches!(r, Err((ErrorCode::NotAllowed, _))));

    // Flat book: re-arm works, strategies stay off until enabled.
    assert!(
        e.execute("test", ClientMsg::Rearm { req_id: 4 })
            .await
            .is_ok()
    );
    assert!(!e.status_dto().kill.triggered);
    assert!(e.strategies_dto().strategies.iter().all(|s| !s.enabled));
}

#[tokio::test]
async fn everything_is_flat_after_flatten_time() {
    let cfg = config("10:40");
    let recs = records(&cfg);
    let limits = RiskLimits {
        entry_end: HhMm(10 * 60 + 25),
        flatten_at: HhMm(10 * 60 + 30),
        // Keep the strategy in a trade until flatten time does the work.
        ..RiskLimits::default()
    };
    let mut e = engine(cfg, limits);
    feed(&mut e, &recs).await;
    assert!(fills(&e) > 0);
    assert_eq!(open_positions(&e), 0);
    let orders = e.orders_dto().orders;
    assert!(orders.iter().all(|o| !o.working));
}

#[tokio::test]
async fn daily_max_loss_fires_kill_switch_that_cannot_rearm_today() {
    let cfg = config("12:00");
    let recs = records(&cfg);
    let limits = RiskLimits {
        // Charges alone exceed this after the first fill.
        daily_max_loss: Paise(100),
        ..RiskLimits::default()
    };
    let mut e = engine(cfg, limits);
    let used = feed_until(&mut e, &recs, |e| e.status_dto().kill.triggered).await;
    assert_eq!(e.status_dto().kill.cause.as_deref(), Some("daily_max_loss"));
    feed(&mut e, &recs[used..used + 30]).await;
    assert_eq!(open_positions(&e), 0);
    let r = e.execute("test", ClientMsg::Rearm { req_id: 1 }).await;
    assert!(matches!(r, Err((ErrorCode::NotAllowed, _))));
}

#[tokio::test]
async fn manual_place_and_exit_go_through_risk() {
    let mut cfg = config("10:30");
    cfg.strategies.momentum.enabled = false;
    let recs = records(&cfg);
    let mut e = engine(cfg, RiskLimits::default());
    // Into the entry window (09:20) with live quotes.
    let used = feed_until(&mut e, &recs, |e| {
        e.now().ist_minute_of_day() >= 9 * 60 + 30
    })
    .await;

    // An option priced Rs 20 to Rs 200 fits the premium limit.
    let chain = e.chain_dto();
    let token = chain
        .rows
        .iter()
        .flat_map(|r| [r.ce.as_ref(), r.pe.as_ref()])
        .flatten()
        .find(|q| q.ask.is_some_and(|a| (2_000..20_000).contains(&a)))
        .expect("an option in range")
        .token;

    // Too many lots: resized to the per-trade max, still sent.
    let r = e
        .execute(
            "test",
            ClientMsg::Place {
                req_id: 1,
                token,
                lots: 5,
                limit: None,
            },
        )
        .await;
    assert!(r.is_ok(), "{r:?}");
    feed(&mut e, &recs[used..used + 5]).await;
    let pos = e.positions_dto();
    let p = pos.positions.iter().find(|p| p.token == token).unwrap();
    assert_eq!(p.lots, 1, "resized to per_trade_max_lots");

    // Only options can be traded.
    let index = 256_265;
    let r = e
        .execute(
            "test",
            ClientMsg::Place {
                req_id: 2,
                token: index,
                lots: 1,
                limit: None,
            },
        )
        .await;
    assert!(matches!(r, Err((ErrorCode::BadRequest, _))));

    assert!(
        e.execute("test", ClientMsg::Exit { req_id: 3, token })
            .await
            .is_ok()
    );
    feed(&mut e, &recs[used + 5..used + 20]).await;
    assert_eq!(open_positions(&e), 0);
    let manual = e
        .orders_dto()
        .orders
        .iter()
        .filter(|o| o.strategy == "manual")
        .count();
    assert!(manual >= 2);
}

#[tokio::test]
async fn pause_blocks_strategy_entries_and_live_mode_is_refused() {
    let cfg = config("11:30");
    let recs = records(&cfg);
    let mut e = engine(cfg, RiskLimits::default());
    assert!(
        e.execute(
            "test",
            ClientMsg::PauseAi {
                req_id: 1,
                paused: true
            }
        )
        .await
        .is_ok()
    );
    feed(&mut e, &recs).await;
    assert_eq!(fills(&e), 0, "paused: no strategy entries");
    let r = e
        .execute(
            "test",
            ClientMsg::SetMode {
                req_id: 2,
                mode: Mode::Live,
            },
        )
        .await;
    assert!(matches!(r, Err((ErrorCode::NotAvailable, _))));
    let r = e
        .execute("test", ClientMsg::KiteLoginUrl { req_id: 3 })
        .await;
    assert!(matches!(r, Err((ErrorCode::NotAvailable, _))));
}

#[tokio::test]
async fn loosening_limits_in_market_hours_needs_flat_book_and_confirm() {
    let cfg = config("10:00");
    let recs = records(&cfg);
    let mut e = engine(cfg, RiskLimits::default());
    feed_until(&mut e, &recs, |e| {
        e.now().ist_minute_of_day() >= 9 * 60 + 30
    })
    .await;
    let looser = RiskLimits {
        per_trade_max_lots: 4,
        ..RiskLimits::default()
    };
    let msg = |confirm| ClientMsg::SetRisk {
        req_id: 1,
        limits: limits_to_dto(&looser),
        confirm,
    };
    assert!(matches!(
        e.execute("test", msg(false)).await,
        Err((ErrorCode::NotAllowed, _))
    ));
    assert!(e.execute("test", msg(true)).await.is_ok());
    assert_eq!(e.risk_dto().limits.per_trade_max_lots, 4);

    // Tightening never needs confirmation.
    let tighter = RiskLimits {
        per_trade_max_lots: 1,
        ..RiskLimits::default()
    };
    let r = e
        .execute(
            "test",
            ClientMsg::SetRisk {
                req_id: 2,
                limits: limits_to_dto(&tighter),
                confirm: false,
            },
        )
        .await;
    assert!(r.is_ok());
}
