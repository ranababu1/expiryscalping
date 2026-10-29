//! Order state machine tests: every transition, duplicate and out-of-order
//! postbacks, timeouts to Unknown and reconcile, partial fills and the
//! 25-modification rule.

use super::*;

const TOK: u32 = 1002;

fn ts(ms: i64) -> Ts {
    Ts::from_millis(1_800_000_000_000 + ms)
}

fn oms() -> Oms {
    Oms::new(KiteLimits::default(), ChargesConfig::default())
}

fn no_charges() -> Oms {
    let c = ChargesConfig {
        brokerage_per_order: Paise::ZERO,
        stt_sell_e8: 0,
        exchange_nse_e8: 0,
        exchange_bse_e8: 0,
        sebi_e8: 0,
        stamp_buy_e8: 0,
        gst_e8: 0,
    };
    Oms::new(KiteLimits::default(), c)
}

fn req(side: Side, lots: u32, px: i64, exit: bool) -> OrderRequest {
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

fn bid(n: u32) -> BrokerOrderId {
    BrokerOrderId(format!("B{n}"))
}

fn fill(n: u32, trade: &str, qty: u32, px: i64) -> BrokerEvent {
    BrokerEvent::Fill {
        id: bid(n),
        trade_id: trade.into(),
        qty,
        price: Paise(px),
        ts: ts(5),
    }
}

/// Creates, sends and opens an order with broker id `B{n}`.
fn open(o: &mut Oms, n: u32, r: OrderRequest) -> OrderId {
    let id = o.create(r, "NFO", 65, ts(0)).unwrap();
    assert_eq!(o.order(id).unwrap().state, OrderState::Created);
    o.mark_sent(id, ts(1)).unwrap();
    assert_eq!(o.order(id).unwrap().state, OrderState::Sent);
    o.on_place_result(id, Ok(bid(n)), ts(2));
    assert_eq!(o.order(id).unwrap().state, OrderState::Open);
    id
}

fn state(o: &Oms, id: OrderId) -> OrderState {
    o.order(id).unwrap().state
}

#[test]
fn created_sent_open_partial_complete() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 2, 10_000, false));
    let up = o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    assert!(matches!(up[0], OmsUpdate::Fill(Fill { qty: 65, .. })));
    assert_eq!(state(&o, id), OrderState::PartiallyFilled);
    o.on_event(&fill(1, "t2", 65, 10_010), ts(4));
    assert_eq!(state(&o, id), OrderState::Complete);
    let m = o.order(id).unwrap();
    assert_eq!((m.filled, m.avg_fill_price), (130, Paise(10_005)));
    // Complete postback after the fills is a no-op on a terminal order.
    assert!(
        o.on_event(&BrokerEvent::Complete { id: bid(1) }, ts(5))
            .is_empty()
    );
    assert_eq!(o.position(TOK).unwrap().qty, 130);
}

#[test]
fn sent_rejected_by_broker() {
    let mut o = oms();
    let id = o
        .create(req(Side::Buy, 1, 10_000, false), "NFO", 65, ts(0))
        .unwrap();
    o.mark_sent(id, ts(1)).unwrap();
    o.on_place_result(id, Err(BrokerError::Rejected("margin".into())), ts(2));
    assert_eq!(state(&o, id), OrderState::Rejected);
    assert!(
        o.order(id)
            .unwrap()
            .message
            .as_deref()
            .unwrap()
            .contains("margin")
    );
    // A late place result does not resurrect it.
    o.on_place_result(id, Ok(bid(1)), ts(3));
    assert_eq!(state(&o, id), OrderState::Rejected);
}

#[test]
fn rejected_postback_after_open() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 10_000, false));
    o.on_event(
        &BrokerEvent::Rejected {
            id: bid(1),
            reason: "RMS".into(),
        },
        ts(3),
    );
    assert_eq!(state(&o, id), OrderState::Rejected);
}

#[test]
fn mark_sent_only_from_created() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 10_000, false));
    assert_eq!(
        o.mark_sent(id, ts(3)),
        Err(OmsError::NotOpen(OrderState::Open))
    );
    assert_eq!(o.mark_sent(OrderId(99), ts(3)), Err(OmsError::UnknownOrder));
}

#[test]
fn modify_pending_then_open() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 9_900, false));
    assert_eq!(o.begin_modify(id, ts(3)), Ok(bid(1)));
    assert_eq!(state(&o, id), OrderState::ModifyPending);
    o.on_modify_result(id, Ok(()), ts(4));
    assert_eq!(state(&o, id), OrderState::ModifyPending);
    o.on_event(
        &BrokerEvent::Modified {
            id: bid(1),
            qty: 65,
            limit: Paise(10_000),
        },
        ts(5),
    );
    assert_eq!(state(&o, id), OrderState::Open);
    assert_eq!(o.order(id).unwrap().limit, Some(Paise(10_000)));
}

#[test]
fn failed_modify_returns_to_previous_state() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 2, 10_000, false));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    o.begin_modify(id, ts(4)).unwrap();
    o.on_modify_result(id, Err(BrokerError::Rejected("bad price".into())), ts(5));
    assert_eq!(state(&o, id), OrderState::PartiallyFilled);
}

#[test]
fn fill_during_modify_and_completion() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 10_000, false));
    o.begin_modify(id, ts(3)).unwrap();
    o.on_event(&fill(1, "t1", 65, 10_000), ts(4));
    assert_eq!(state(&o, id), OrderState::Complete);
    // The modify reply arriving later does not change a complete order.
    o.on_modify_result(id, Err(BrokerError::NotOpen), ts(5));
    assert_eq!(state(&o, id), OrderState::Complete);
}

#[test]
fn twenty_five_modifications_then_cancel_and_replace() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 9_000, false));
    for i in 0..25 {
        let now = ts(10 + i * 200); // stay under the per-second cap
        o.begin_modify(id, now).unwrap();
        o.on_event(
            &BrokerEvent::Modified {
                id: bid(1),
                qty: 65,
                limit: Paise(9_000 + i),
            },
            now,
        );
    }
    assert_eq!(o.order(id).unwrap().modifications, 25);
    assert_eq!(
        o.begin_modify(id, ts(10_000)),
        Err(OmsError::ModifyLimit(25))
    );
    // Cancel is still possible.
    assert!(o.begin_cancel(id, ts(10_001)).is_ok());
}

#[test]
fn modify_only_when_open() {
    let mut o = oms();
    let id = o
        .create(req(Side::Buy, 1, 10_000, false), "NFO", 65, ts(0))
        .unwrap();
    assert_eq!(
        o.begin_modify(id, ts(1)),
        Err(OmsError::NotOpen(OrderState::Created))
    );
    o.mark_sent(id, ts(1)).unwrap();
    assert_eq!(
        o.begin_modify(id, ts(1)),
        Err(OmsError::NotOpen(OrderState::Sent))
    );
}

#[test]
fn cancel_pending_then_cancelled() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 9_000, false));
    assert_eq!(o.begin_cancel(id, ts(3)), Ok(bid(1)));
    assert_eq!(state(&o, id), OrderState::CancelPending);
    o.on_cancel_result(id, Ok(()), ts(4));
    assert_eq!(state(&o, id), OrderState::CancelPending);
    o.on_event(&BrokerEvent::Cancelled { id: bid(1) }, ts(5));
    assert_eq!(state(&o, id), OrderState::Cancelled);
    assert_eq!(
        o.begin_cancel(id, ts(6)),
        Err(OmsError::NotOpen(OrderState::Cancelled))
    );
}

#[test]
fn failed_cancel_reverts_but_not_open_waits_for_event() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 9_000, false));
    o.begin_cancel(id, ts(3)).unwrap();
    o.on_cancel_result(id, Err(BrokerError::Unknown("timeout".into())), ts(4));
    assert_eq!(state(&o, id), OrderState::Open);

    o.begin_cancel(id, ts(5)).unwrap();
    o.on_cancel_result(id, Err(BrokerError::NotOpen), ts(6));
    assert_eq!(state(&o, id), OrderState::CancelPending);
    // It had filled: the fill wins.
    o.on_event(&fill(1, "t1", 65, 9_000), ts(7));
    assert_eq!(state(&o, id), OrderState::Complete);
}

#[test]
fn duplicate_fill_postback_is_ignored() {
    let mut o = no_charges();
    let id = open(&mut o, 1, req(Side::Buy, 2, 10_000, false));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    assert!(o.on_event(&fill(1, "t1", 65, 10_000), ts(4)).is_empty());
    assert_eq!(o.order(id).unwrap().filled, 65);
    assert_eq!(o.position(TOK).unwrap().qty, 65);
}

#[test]
fn complete_before_last_fill() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 2, 10_000, false));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    o.on_event(&BrokerEvent::Complete { id: bid(1) }, ts(4));
    assert_eq!(state(&o, id), OrderState::PartiallyFilled);
    o.on_event(&fill(1, "t2", 65, 10_000), ts(5));
    assert_eq!(state(&o, id), OrderState::Complete);
}

#[test]
fn fill_after_cancel_is_still_booked() {
    let mut o = no_charges();
    let id = open(&mut o, 1, req(Side::Buy, 2, 10_000, false));
    o.on_event(&BrokerEvent::Cancelled { id: bid(1) }, ts(3));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(4));
    assert_eq!(state(&o, id), OrderState::Cancelled);
    assert_eq!(o.position(TOK).unwrap().qty, 65);
}

#[test]
fn terminal_states_do_not_regress() {
    let mut o = oms();
    let id = open(&mut o, 1, req(Side::Buy, 1, 10_000, false));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    o.on_event(
        &BrokerEvent::Rejected {
            id: bid(1),
            reason: "late".into(),
        },
        ts(4),
    );
    o.on_event(&BrokerEvent::Cancelled { id: bid(1) }, ts(4));
    assert_eq!(state(&o, id), OrderState::Complete);
}

#[test]
fn events_for_unknown_broker_ids_are_ignored() {
    let mut o = oms();
    assert!(o.on_event(&fill(42, "x", 65, 1), ts(1)).is_empty());
    assert!(o.positions().next().is_none());
}

#[test]
fn network_error_goes_unknown_and_reconciles_found() {
    let mut o = no_charges();
    let id = o
        .create(req(Side::Buy, 1, 10_000, false), "NFO", 65, ts(0))
        .unwrap();
    o.mark_sent(id, ts(1)).unwrap();
    o.on_place_result(id, Err(BrokerError::Unknown("502".into())), ts(2));
    assert_eq!(state(&o, id), OrderState::Unknown);
    assert!(o.has_unknown());
    let tag = o.order(id).unwrap().tag.clone();
    let book = vec![BrokerOrder {
        id: bid(7),
        tag,
        token: TOK,
        side: Side::Buy,
        qty: 65,
        filled: 65,
        limit: Some(Paise(10_000)),
        order_type: OrderType::Limit,
        status: BrokerOrderStatus::Complete,
    }];
    let up = o.reconcile(&book, ts(3));
    assert!(up.iter().any(|u| matches!(u, OmsUpdate::Fill(_))));
    assert_eq!(state(&o, id), OrderState::Complete);
    assert_eq!(o.order(id).unwrap().broker_id, Some(bid(7)));
    assert_eq!(o.position(TOK).unwrap().qty, 65);
    assert!(!o.has_unknown());
    // Later postbacks for the reconciled order are matched.
    assert!(
        o.on_event(&BrokerEvent::Complete { id: bid(7) }, ts(4))
            .is_empty()
    );
}

#[test]
fn timeout_goes_unknown_and_reconciles_not_found() {
    let mut o = oms();
    let id = o
        .create(req(Side::Buy, 1, 10_000, false), "NFO", 65, ts(0))
        .unwrap();
    o.mark_sent(id, ts(0)).unwrap();
    assert!(o.check_timeouts(ts(1_999), 2_000).is_empty());
    assert_eq!(o.check_timeouts(ts(2_000), 2_000), vec![id]);
    assert_eq!(state(&o, id), OrderState::Unknown);
    o.reconcile(&[], ts(2_001));
    assert_eq!(state(&o, id), OrderState::Rejected);
}

#[test]
fn reconcile_open_with_partial() {
    let mut o = no_charges();
    let id = o
        .create(req(Side::Buy, 2, 10_000, false), "NFO", 65, ts(0))
        .unwrap();
    o.mark_sent(id, ts(0)).unwrap();
    o.check_timeouts(ts(5_000), 2_000);
    let tag = o.order(id).unwrap().tag.clone();
    let book = vec![BrokerOrder {
        id: bid(3),
        tag,
        token: TOK,
        side: Side::Buy,
        qty: 130,
        filled: 65,
        limit: Some(Paise(10_000)),
        order_type: OrderType::Limit,
        status: BrokerOrderStatus::Open,
    }];
    o.reconcile(&book, ts(5_001));
    assert_eq!(state(&o, id), OrderState::PartiallyFilled);
    o.on_event(&fill(3, "t9", 65, 10_000), ts(5_002));
    assert_eq!(state(&o, id), OrderState::Complete);
}

#[test]
fn kite_rate_limit_blocks_new_orders() {
    let mut o = oms();
    for i in 0..9 {
        let id = o
            .create(req(Side::Buy, 1, 9_000, false), "NFO", 65, ts(i))
            .unwrap();
        o.mark_sent(id, ts(i)).unwrap();
    }
    assert_eq!(
        o.create(req(Side::Buy, 1, 9_000, false), "NFO", 65, ts(500)),
        Err(OmsError::RateLimited)
    );
    assert!(
        o.create(req(Side::Buy, 1, 9_000, false), "NFO", 65, ts(1_000))
            .is_ok()
    );
    assert_eq!(o.calls_today(), 9);
}

#[test]
fn tags_are_short_alphanumeric_and_unique() {
    assert_eq!(make_tag("momentum", 12), "ESMOME12");
    assert_eq!(make_tag("x-y", 1), "ESXY1");
    let t = make_tag("abcd", u64::MAX);
    assert!(t.len() <= 20 && t.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn snapshot_and_trade_closed_net_of_charges() {
    let mut o = oms();
    let e = open(&mut o, 1, req(Side::Buy, 1, 10_000, false));
    o.on_event(&fill(1, "t1", 65, 10_000), ts(3));
    assert_eq!(state(&o, e), OrderState::Complete);
    let x = open(&mut o, 2, req(Side::Sell, 1, 11_000, true));
    let snap = o.snapshot(|_| Some(Paise(10_500)));
    assert_eq!(
        snap.positions,
        vec![PositionView {
            token: TOK,
            qty: 65,
            pending_exit_qty: 65
        }]
    );
    assert!(snap.pending_entry_tokens.is_empty());
    assert_eq!(snap.unrealised, Paise(65 * 500));
    let up = o.on_event(&fill(2, "t2", 65, 11_000), ts(6));
    assert_eq!(state(&o, x), OrderState::Complete);
    let pnl = up
        .iter()
        .find_map(|u| match u {
            OmsUpdate::TradeClosed { pnl, .. } => Some(*pnl),
            _ => None,
        })
        .unwrap();
    // Gross Rs 650 minus charges on both legs (2650 + 3371 paise on these
    // turnovers, see es-broker charges tests for the arithmetic).
    let buy = ChargesConfig::default().on_fill(Side::Buy, Paise(650_000), "NFO", true);
    let sell = ChargesConfig::default().on_fill(Side::Sell, Paise(715_000), "NFO", true);
    assert_eq!(pnl, Paise(65_000) - buy - sell);
    let snap = o.snapshot(|_| None);
    assert_eq!(snap.realised, pnl);
    assert!(snap.view(Paise::ZERO, false).is_flat());
}

#[test]
fn pending_entries_are_reported() {
    let mut o = oms();
    open(&mut o, 1, req(Side::Buy, 1, 9_000, false));
    let snap = o.snapshot(|_| None);
    assert_eq!(snap.pending_entry_tokens, vec![TOK]);
    assert_eq!(snap.view(Paise::ZERO, false).open_count(), 1);
    assert_eq!(o.working().len(), 1);
}
