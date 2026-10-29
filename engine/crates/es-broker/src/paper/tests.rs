//! Paper fills: depth walking, partials, latency, slippage, protection.

use super::*;
use es_core::DepthLevel;

const TOK: u32 = 1002;

fn ts(ms: i64) -> Ts {
    Ts::from_millis(1_800_000_000_000 + ms)
}

fn lv(price: i64, qty: u32) -> DepthLevel {
    DepthLevel {
        qty,
        price: Paise(price),
        orders: 1,
    }
}

/// Asks 100.00 x 65, 100.05 x 65, 100.10 x 130. Bids 99.95 x 65, 99.90 x 65.
fn tick(ms: i64) -> Tick {
    let mut d = Depth5::default();
    d.asks[0] = lv(10_000, 65);
    d.asks[1] = lv(10_005, 65);
    d.asks[2] = lv(10_010, 130);
    d.bids[0] = lv(9_995, 65);
    d.bids[1] = lv(9_990, 65);
    Tick {
        token: TOK,
        ltp: Paise(10_000),
        depth: Some(d),
        recv: ts(ms),
        ..Tick::default()
    }
}

fn limit(side: Side, lots: u32, px: i64, exit: bool) -> OrderRequest {
    OrderRequest::for_tests(
        TOK,
        side,
        lots,
        65,
        OrderType::Limit,
        Some(Paise(px)),
        exit,
        ts(0),
    )
}

fn buy(lots: u32, px: i64) -> OrderRequest {
    limit(Side::Buy, lots, px, false)
}

fn instant() -> PaperBroker {
    PaperBroker::new(PaperConfig {
        latency_ms: 0,
        ..Default::default()
    })
}

fn events(b: &PaperBroker) -> Vec<BrokerEvent> {
    let mut v = Vec::new();
    b.drain_events(&mut v);
    v
}

fn fills(ev: &[BrokerEvent]) -> Vec<(u32, i64)> {
    ev.iter()
        .filter_map(|e| match e {
            BrokerEvent::Fill { qty, price, .. } => Some((*qty, price.0)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn no_fill_before_latency_then_fills_at_best_ask() {
    let b = PaperBroker::new(PaperConfig {
        latency_ms: 50,
        ..Default::default()
    });
    b.on_tick(&tick(0));
    let id = b.place(&buy(1, 10_000)).await.unwrap();
    b.on_tick(&tick(49));
    assert!(fills(&events(&b)).is_empty());
    b.on_tick(&tick(50));
    let ev = events(&b);
    assert_eq!(fills(&ev), vec![(65, 10_000)]);
    assert!(ev.contains(&BrokerEvent::Complete { id }));
}

#[tokio::test]
async fn walks_depth_up_to_limit_and_leaves_rest_open() {
    let b = instant();
    b.on_tick(&tick(0));
    // 3 lots = 195, limit 100.05 reaches the first two levels only.
    let id = b.place(&buy(3, 10_005)).await.unwrap();
    let ev = events(&b);
    assert_eq!(fills(&ev), vec![(65, 10_000), (65, 10_005)]);
    assert!(!ev.iter().any(|e| matches!(e, BrokerEvent::Complete { .. })));
    let o = b.orders().await.unwrap()[0].clone();
    assert_eq!((o.filled, o.status), (130, BrokerOrderStatus::Open));
    // The same snapshot is not used twice.
    b.advance(ts(100));
    assert!(fills(&events(&b)).is_empty());
    // A new tick fills the rest at the best ask.
    b.on_tick(&tick(200));
    let ev = events(&b);
    assert_eq!(fills(&ev), vec![(65, 10_000)]);
    assert!(ev.contains(&BrokerEvent::Complete { id }));
}

#[tokio::test]
async fn limit_below_ask_does_not_fill() {
    let b = instant();
    b.on_tick(&tick(0));
    b.place(&buy(1, 9_995)).await.unwrap();
    b.on_tick(&tick(10));
    assert!(fills(&events(&b)).is_empty());
}

#[tokio::test]
async fn slippage_is_against_us() {
    let b = PaperBroker::new(PaperConfig {
        latency_ms: 0,
        slippage_paise: 5,
        ..Default::default()
    });
    b.on_tick(&tick(0));
    b.place(&buy(1, 10_100)).await.unwrap();
    assert_eq!(fills(&events(&b)), vec![(65, 10_005)]);
    b.on_tick(&tick(10));
    b.place(&limit(Side::Sell, 1, 9_900, true)).await.unwrap();
    assert_eq!(fills(&events(&b)), vec![(65, 9_990)]);
    assert!(b.positions().await.unwrap().is_empty());
}

#[tokio::test]
async fn market_order_bounded_by_protection() {
    let b = instant();
    let mut t = tick(0);
    if let Some(d) = t.depth.as_mut() {
        d.asks[1] = lv(10_300, 65); // exactly 3% above 100.00: allowed
        d.asks[2] = lv(10_305, 130); // beyond protection
    }
    b.on_tick(&t);
    let m = OrderRequest::for_tests(TOK, Side::Buy, 3, 65, OrderType::Market, None, false, ts(0));
    b.place(&m).await.unwrap();
    assert_eq!(fills(&events(&b)), vec![(65, 10_000), (65, 10_300)]);
}

#[tokio::test]
async fn positions_and_margins_track_fills() {
    let b = instant();
    b.on_tick(&tick(0));
    b.place(&buy(1, 10_000)).await.unwrap();
    assert_eq!(
        b.positions().await.unwrap(),
        vec![Position {
            token: TOK,
            qty: 65
        }]
    );
    let m = b.margins().await.unwrap();
    assert_eq!(m.available, Paise::from_rupees(100_000) - Paise(650_000));
}

#[tokio::test]
async fn modify_and_cancel() {
    let b = instant();
    b.on_tick(&tick(0));
    let id = b.place(&buy(1, 9_900)).await.unwrap();
    // Raising the limit to the ask fills immediately.
    b.modify(&id, ApprovedModify::for_tests(65, Paise(10_000)))
        .await
        .unwrap();
    let ev = events(&b);
    assert!(matches!(ev[0], BrokerEvent::Modified { qty: 65, .. }));
    assert_eq!(fills(&ev), vec![(65, 10_000)]);
    assert_eq!(b.cancel(&id).await, Err(BrokerError::NotOpen));

    let id2 = b.place(&buy(1, 9_000)).await.unwrap();
    b.cancel(&id2).await.unwrap();
    assert_eq!(events(&b), vec![BrokerEvent::Cancelled { id: id2.clone() }]);
    assert_eq!(b.cancel(&id2).await, Err(BrokerError::NotOpen));
    let bogus = BrokerOrderId("nope".into());
    assert!(matches!(
        b.cancel(&bogus).await,
        Err(BrokerError::UnknownOrder(_))
    ));
}

#[tokio::test]
async fn modify_below_filled_is_rejected() {
    let b = instant();
    b.on_tick(&tick(0));
    let id = b.place(&buy(3, 10_000)).await.unwrap(); // fills 65 of 195
    let r = b
        .modify(&id, ApprovedModify::for_tests(0, Paise(10_000)))
        .await;
    assert!(matches!(r, Err(BrokerError::Rejected(_))));
    // Reducing to the filled quantity completes the order.
    b.modify(&id, ApprovedModify::for_tests(65, Paise(10_000)))
        .await
        .unwrap();
    assert!(events(&b).contains(&BrokerEvent::Complete { id }));
}

#[tokio::test]
async fn kite_stub_is_not_available() {
    let k = crate::kite::KiteBroker;
    assert!(matches!(
        k.margins().await,
        Err(BrokerError::NotAvailable(_))
    ));
    assert!(matches!(
        k.orders().await,
        Err(BrokerError::NotAvailable(_))
    ));
}
