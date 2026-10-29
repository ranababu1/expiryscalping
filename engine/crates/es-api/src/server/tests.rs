use super::*;
use tokio_tungstenite::connect_async;

type Client =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Harness {
    url: String,
    token: String,
    publisher: Publisher,
}

/// Starts a server with one device and a fake engine that acks `Kill` and
/// rejects everything else.
async fn start() -> Harness {
    let mut store = DeviceStore::default();
    let token = store.add("desktop", 0).unwrap();
    let devices = Arc::new(RwLock::new(store));
    let (tx, mut rx) = mpsc::channel::<ApiCommand>(16);
    let publisher = Publisher::new();
    publisher.publish(ServerMsg::Alert(AlertDto {
        level: AlertLevel::Info,
        code: "x".into(),
        message: "events are not replayed".into(),
        ts: 0,
    }));
    publisher.publish(ServerMsg::Strategies(StrategiesDto {
        strategies: vec![],
        ai_paused: false,
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(serve(
        listener,
        devices,
        tx,
        publisher.clone(),
        "0.0.0".into(),
    ));
    tokio::spawn(async move {
        while let Some(c) = rx.recv().await {
            let reply = match c.msg {
                ClientMsg::Kill { req_id } => ServerMsg::Ack {
                    req_id,
                    message: Some(format!("kill from {}", c.device)),
                },
                m => error(m.req_id(), ErrorCode::NotAvailable, "no"),
            };
            let _ = c.reply.send(reply);
        }
    });
    Harness {
        url,
        token,
        publisher,
    }
}

async fn send(c: &mut Client, v: serde_json::Value) {
    c.send(Message::text(v.to_string())).await.unwrap();
}

async fn recv(c: &mut Client) -> Option<ServerMsg> {
    loop {
        match tokio::time::timeout(Duration::from_secs(2), c.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return Some(serde_json::from_str(&t).unwrap()),
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) | Err(_) => {
                return None;
            }
            Ok(Some(Ok(_))) => continue,
        }
    }
}

async fn authed(h: &Harness) -> Client {
    let (mut c, _) = connect_async(&h.url).await.unwrap();
    send(
        &mut c,
        serde_json::json!({"type": "auth", "token": h.token}),
    )
    .await;
    assert!(
        matches!(recv(&mut c).await, Some(ServerMsg::Hello { device, .. }) if device == "desktop")
    );
    // Latest snapshot, but not past events.
    assert!(matches!(recv(&mut c).await, Some(ServerMsg::Strategies(_))));
    c
}

#[tokio::test]
async fn bad_token_is_rejected_and_closed() {
    let h = start().await;
    let (mut c, _) = connect_async(&h.url).await.unwrap();
    send(
        &mut c,
        serde_json::json!({"type": "auth", "token": "es_nope"}),
    )
    .await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            code: ErrorCode::Unauthorized,
            ..
        })
    ));
    assert!(recv(&mut c).await.is_none());
}

#[tokio::test]
async fn command_before_auth_is_rejected() {
    let h = start().await;
    let (mut c, _) = connect_async(&h.url).await.unwrap();
    send(&mut c, serde_json::json!({"type": "kill", "req_id": 1})).await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            code: ErrorCode::Unauthorized,
            ..
        })
    ));
    assert!(recv(&mut c).await.is_none());
}

#[tokio::test]
async fn token_with_one_char_changed_is_rejected() {
    let h = start().await;
    let mut bad = h.token.clone();
    let last = if bad.ends_with('0') { '1' } else { '0' };
    bad.pop();
    bad.push(last);
    let (mut c, _) = connect_async(&h.url).await.unwrap();
    send(&mut c, serde_json::json!({"type": "auth", "token": bad})).await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            code: ErrorCode::Unauthorized,
            ..
        })
    ));
}

#[tokio::test]
async fn commands_get_ack_or_error_with_req_id() {
    let h = start().await;
    let mut c = authed(&h).await;
    send(&mut c, serde_json::json!({"type": "kill", "req_id": 7})).await;
    assert_eq!(
        recv(&mut c).await,
        Some(ServerMsg::Ack {
            req_id: 7,
            message: Some("kill from desktop".into())
        })
    );
    send(&mut c, serde_json::json!({"type": "rearm", "req_id": 8})).await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            req_id: Some(8),
            code: ErrorCode::NotAvailable,
            ..
        })
    ));
    send(&mut c, serde_json::json!({"type": "ping", "req_id": 9})).await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Ack { req_id: 9, .. })
    ));
}

#[tokio::test]
async fn malformed_messages_get_bad_request() {
    let h = start().await;
    let mut c = authed(&h).await;
    send(
        &mut c,
        serde_json::json!({"type": "place", "req_id": 3, "token": "x"}),
    )
    .await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            req_id: Some(3),
            code: ErrorCode::BadRequest,
            ..
        })
    ));
    c.send(Message::text("not json")).await.unwrap();
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            req_id: None,
            code: ErrorCode::BadRequest,
            ..
        })
    ));
    send(
        &mut c,
        serde_json::json!({"type": "kill", "req_id": 4, "extra": 1}),
    )
    .await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Error {
            req_id: Some(4),
            code: ErrorCode::BadRequest,
            ..
        })
    ));
}

#[tokio::test]
async fn updates_respect_subscriptions() {
    let h = start().await;
    let mut c = authed(&h).await;
    send(
        &mut c,
        serde_json::json!({"type": "subscribe", "req_id": 1, "streams": ["alerts"]}),
    )
    .await;
    assert!(matches!(
        recv(&mut c).await,
        Some(ServerMsg::Ack { req_id: 1, .. })
    ));
    h.publisher.publish(ServerMsg::Strategies(StrategiesDto {
        strategies: vec![],
        ai_paused: true,
    }));
    h.publisher.publish(ServerMsg::Alert(AlertDto {
        level: AlertLevel::Warn,
        code: "StaleData".into(),
        message: "feed stale".into(),
        ts: 1,
    }));
    match recv(&mut c).await {
        Some(ServerMsg::Alert(a)) => assert_eq!(a.code, "StaleData"),
        other => panic!("expected the alert only, got {other:?}"),
    }
}
