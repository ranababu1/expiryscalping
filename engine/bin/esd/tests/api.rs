//! The desktop app's path end to end: esd serving the client API over a real
//! WebSocket while it paper trades a simulated day.

use es_api::{ClientMsg, DeviceStore, Publisher, ServerMsg};
use es_core::Date;
use es_risk::RiskLimits;
use esd::config::EngineConfig;
use esd::{Engine, EngineOptions, Journal, RunOptions};
use futures_util::{SinkExt, StreamExt};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const DAY: Date = Date {
    year: 2026,
    month: 10,
    day: 29,
};

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send(ws: &mut Ws, m: &ClientMsg) {
    let t = serde_json::to_string(m).unwrap();
    ws.send(Message::text(t)).await.unwrap();
}

/// Next server message matching `pred`, within 10 s.
async fn next_where(ws: &mut Ws, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let m = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("timed out waiting for a message")
            .expect("socket closed")
            .unwrap();
        if let Message::Text(t) = m {
            let msg: ServerMsg = serde_json::from_str(&t).unwrap();
            if pred(&msg) {
                return msg;
            }
        }
    }
}

#[tokio::test]
async fn app_connects_sees_snapshots_and_presses_kill() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/engine.toml");
    let mut cfg = EngineConfig::load(&path).unwrap();
    cfg.sim.end = "10:30".into();
    let sim = es_replay::Simulator::new(&cfg.sim, DAY).unwrap();

    let mut store = DeviceStore::default();
    let token = store.add("desktop", 0).unwrap();
    let publisher = Publisher::new();
    let opts = EngineOptions {
        source: "test sim".into(),
        version: "0.0.0-test".into(),
        data_dir: None,
    };
    let engine = Engine::new(
        cfg,
        RiskLimits::default(),
        opts,
        Journal::in_memory(),
        Some(publisher.clone()),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    tokio::spawn(es_api::serve(
        listener,
        Arc::new(RwLock::new(store)),
        cmd_tx,
        publisher,
        "0.0.0-test".into(),
    ));
    // Fast but not instant, so the test can act mid-session.
    let (tx, rx) = mpsc::channel(1024);
    tokio::spawn(es_replay::pace(sim, 600.0, tx));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let run = tokio::spawn(esd::run(
        engine,
        rx,
        cmd_rx,
        RunOptions {
            publish_every: Duration::from_millis(50),
            recorder: None,
            exit_when_done: false,
        },
        async {
            let _ = stop_rx.await;
        },
    ));

    // A bad token is refused.
    let (mut bad, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();
    send(
        &mut bad,
        &ClientMsg::Auth {
            token: "es_wrong".into(),
        },
    )
    .await;
    let r = tokio::time::timeout(Duration::from_secs(5), bad.next()).await;
    let refused = match r {
        Ok(None) | Ok(Some(Err(_))) => true,
        Ok(Some(Ok(Message::Close(_)))) => true,
        Ok(Some(Ok(Message::Text(t)))) => t.contains("unauthorized"),
        _ => false,
    };
    assert!(refused, "bad token must be refused");

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();
    send(&mut ws, &ClientMsg::Auth { token }).await;
    let hello = next_where(&mut ws, |m| matches!(m, ServerMsg::Hello { .. })).await;
    assert!(matches!(hello, ServerMsg::Hello { ref device, .. } if device == "desktop"));

    // Snapshots stream in: a chain with rows and a status in paper mode.
    let chain = next_where(
        &mut ws,
        |m| matches!(m, ServerMsg::Chain(c) if !c.rows.is_empty()),
    )
    .await;
    if let ServerMsg::Chain(c) = chain {
        assert_eq!(c.underlying, "NIFTY");
    }
    let status = next_where(&mut ws, |m| matches!(m, ServerMsg::Status(_))).await;
    if let ServerMsg::Status(s) = status {
        assert_eq!(s.mode, es_api::Mode::Paper);
        assert!(!s.kill.triggered);
    }

    // The big red button: an ack, an alert and a status showing it on, in
    // whatever order they arrive.
    send(&mut ws, &ClientMsg::Kill { req_id: 7 }).await;
    let (mut acked, mut alerted, mut shown) = (false, false, false);
    while !(acked && alerted && shown) {
        match next_where(&mut ws, |_| true).await {
            ServerMsg::Ack { req_id: 7, .. } => acked = true,
            ServerMsg::Error {
                req_id: Some(7),
                message,
                ..
            } => panic!("kill refused: {message}"),
            ServerMsg::Alert(a) if a.code == "kill_switch" => alerted = true,
            ServerMsg::Status(s) if s.kill.triggered => shown = true,
            _ => {}
        }
    }

    let _ = stop_tx.send(());
    let engine = run.await.unwrap();
    assert!(engine.status_dto().kill.triggered);
}
