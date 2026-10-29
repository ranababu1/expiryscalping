use super::*;

#[test]
fn exit_always_allowed_even_when_every_entry_rule_is_breached() {
    let mut limits = RiskLimits::default();
    limits.max_trades_per_day = 1;
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    r.trigger_kill(KillCause::Manual, t(0));
    let mut b = Book::long(TOKEN, 65);
    b.realised = Paise::from_rupees(-50_000);
    b.feed_stale = true;
    b.funds = Paise::ZERO;
    let mut i = inst();
    i.age_ms = None;
    i.expiry = None;
    i.best_bid = Some(Paise(5_000)); // huge spread
    for _ in 0..10 {
        // past the rate cap, at 15:29, after the entry window
        let d = r.check(&exit(), &i, &b.view(), at(15, 29));
        let RiskDecision::Approve(o) = d else {
            panic!("{d:?}")
        };
        assert!(o.is_exit());
        assert_eq!(o.side(), Side::Sell);
        assert_eq!(o.qty(), 65);
        // best bid 50.00 minus 2 ticks
        assert_eq!(o.limit(), Some(Paise(4_990)));
    }
}

#[test]
fn exit_without_bid_is_market_with_protection() {
    let mut r = engine();
    let mut i = inst();
    i.best_bid = None;
    let d = r.check(&exit(), &i, &Book::long(TOKEN, 130).view(), t(0));
    let RiskDecision::Approve(o) = d else {
        panic!("{d:?}")
    };
    assert_eq!(o.order_type(), es_core::OrderType::Market);
    assert_eq!(o.market_protection_pct(), 3);
    assert_eq!(o.lots(), 2);
}

#[test]
fn exit_needs_a_position_and_respects_pending_exits() {
    let mut r = engine();
    assert_eq!(
        rejected(&r.check(&exit(), &inst(), &Book::flat().view(), t(0))),
        Some(RejectReason::NoPosition)
    );
    let mut b = Book::long(TOKEN, 130);
    b.positions[0].pending_exit_qty = 65;
    let d = r.check(&exit(), &inst(), &b.view(), t(0));
    assert_eq!(d.order().unwrap().qty(), 65, "only the uncovered lot");
    b.positions[0].pending_exit_qty = 130;
    assert_eq!(
        rejected(&r.check(&exit(), &inst(), &b.view(), t(0))),
        Some(RejectReason::ExitAlreadyPending)
    );
}

#[test]
fn reduce_to() {
    let mut r = engine();
    let mut s = exit();
    s.intent = Intent::ReduceTo(1);
    let d = r.check(&s, &inst(), &Book::long(TOKEN, 195).view(), t(0));
    assert_eq!(d.order().unwrap().lots(), 2);
}

#[test]
fn kill_switch_blocks_entries_and_needs_flat_rearm() {
    let mut r = engine();
    assert!(r.trigger_kill(KillCause::Manual, t(0)));
    assert!(!r.trigger_kill(KillCause::Manual, t(1)), "idempotent");
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &Book::flat().view(), t(0))),
        Some(RejectReason::KillSwitch)
    );
    assert!(r.rearm(&Book::long(TOKEN, 65).view(), t(0)).is_err());
    assert!(r.rearm(&Book::flat().view(), t(0)).is_ok());
    assert!(!r.kill_state().is_triggered());
    assert!(rejected(&r.check(&enter(1), &inst(), &Book::flat().view(), t(0))).is_none());
}

#[test]
fn monitor_fires_kill_on_daily_loss_then_asks_to_flatten() {
    let mut r = engine();
    let mut b = Book::long(TOKEN, 65);
    b.unrealised = Paise(-199_999);
    assert_eq!(r.monitor(&b.view(), t(0)), None);
    b.unrealised = Paise(-200_000);
    assert_eq!(
        r.monitor(&b.view(), t(1)),
        Some(RiskAction::KillSwitch(KillCause::DailyMaxLoss))
    );
    assert_eq!(r.monitor(&b.view(), t(2)), Some(RiskAction::Flatten));
    // daily loss kill cannot be re-armed the same day, even when flat
    assert!(r.rearm(&Book::flat().view(), t(3)).is_err());
    // next day it can
    assert!(r.rearm(&Book::flat().view(), t(24 * 3_600_000)).is_ok());
}

#[test]
fn monitor_flattens_at_cutoff() {
    let mut r = engine();
    let b = Book::long(TOKEN, 65);
    assert_eq!(r.monitor(&b.view(), at(15, 14)), None);
    assert_eq!(r.monitor(&b.view(), at(15, 15)), Some(RiskAction::Flatten));
    assert_eq!(r.monitor(&Book::flat().view(), at(15, 15)), None);
}

#[test]
fn market_exit_for_escalation() {
    let mut r = engine();
    let d = r.check_market_exit("kill", &inst(), &Book::long(TOKEN, 65).view(), t(0));
    let RiskDecision::Approve(o) = d else {
        panic!("{d:?}")
    };
    assert_eq!(o.order_type(), es_core::OrderType::Market);
    assert_eq!(o.reason(), ReasonCode::KillSwitch);
}

#[test]
fn modify_rules() {
    let mut r = engine();
    let entry = OpenOrderView {
        is_exit: false,
        side: Side::Buy,
        lot_size: LOT,
    };
    let exit = OpenOrderView {
        is_exit: true,
        side: Side::Sell,
        lot_size: LOT,
    };
    let m = r.check_modify(&entry, 1, Paise(10_000), t(0)).unwrap();
    assert_eq!((m.qty(), m.limit()), (65, Paise(10_000)));
    assert_eq!(
        r.check_modify(&entry, 2, Paise(10_000), t(0)),
        Err(RejectReason::ResizedToMaxLots)
    );
    // 1 lot at Rs 231 = 15,015 > 15,000
    assert_eq!(
        r.check_modify(&entry, 1, Paise(23_100), t(0)),
        Err(RejectReason::PremiumTooHigh)
    );
    assert_eq!(
        r.check_modify(&entry, 0, Paise(100), t(0)),
        Err(RejectReason::ZeroQuantity)
    );
    r.trigger_kill(KillCause::Manual, t(0));
    assert_eq!(
        r.check_modify(&entry, 1, Paise(100), t(0)),
        Err(RejectReason::KillSwitch)
    );
    // exits can always be modified, any size
    assert!(r.check_modify(&exit, 5, Paise(50_000), t(0)).is_ok());
}
