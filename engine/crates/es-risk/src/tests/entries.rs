use super::*;

#[test]
fn approves_a_normal_entry_as_marketable_limit() {
    let mut r = engine();
    let d = r.check(&enter(1), &inst(), &Book::flat().view(), t(0));
    let RiskDecision::Approve(o) = d else {
        panic!("{d:?}")
    };
    assert_eq!(o.qty(), 65);
    assert_eq!(o.lots(), 1);
    assert_eq!(o.side(), Side::Buy);
    // ask 100.00 + 2 ticks of 0.05
    assert_eq!(o.limit(), Some(Paise(10_010)));
    assert!(!o.is_exit());
}

#[test]
fn daily_max_loss_boundary_and_kill() {
    let mut r = engine();
    let mut b = Book::flat();
    b.realised = Paise(-199_999); // one paisa inside the Rs 2,000 limit
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))).is_none());
    let mut r = engine();
    b.realised = Paise(-100_000);
    b.unrealised = Paise(-100_000); // exactly at the limit
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))),
        Some(RejectReason::DailyMaxLoss)
    );
    assert!(r.kill_state().is_triggered());
}

#[test]
fn per_trade_max_lots_resizes() {
    let mut r = engine();
    let d = r.check(&enter(3), &inst(), &Book::flat().view(), t(0));
    let RiskDecision::Resize(o, why) = d else {
        panic!("{d:?}")
    };
    assert_eq!(o.lots(), 1);
    assert_eq!(why, RejectReason::ResizedToMaxLots);
}

#[test]
fn per_trade_max_premium_boundary() {
    let mut limits = RiskLimits::default();
    limits.per_trade_max_lots = 10;
    // price with buffer is 100.10, lot cost = 100.10 * 65 = 6,506.50
    limits.per_trade_max_premium = Paise(650_650 * 2); // exactly two lots
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    let d = r.check(&enter(5), &inst(), &Book::flat().view(), t(0));
    let RiskDecision::Resize(o, why) = d else {
        panic!("{d:?}")
    };
    assert_eq!((o.lots(), why), (2, RejectReason::ResizedToMaxPremium));

    limits.per_trade_max_premium = Paise(650_650 - 1); // one paisa under one lot
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &Book::flat().view(), t(0))),
        Some(RejectReason::PremiumTooHigh)
    );
}

#[test]
fn max_open_positions() {
    let mut r = engine();
    let mut b = Book::long(7, 65);
    b.pending.push(8); // working entry on another instrument
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))),
        Some(RejectReason::MaxOpenPositions)
    );
    // one below the limit passes
    let b = Book::long(7, 65);
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))).is_none());
}

#[test]
fn max_trades_per_day_and_reset_next_day() {
    let mut limits = RiskLimits::default();
    limits.max_trades_per_day = 2;
    limits.max_orders_per_sec = 10;
    limits.max_orders_per_min = 100;
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    let b = Book::flat();
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))).is_none());
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(1_000))).is_none());
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(2_000))),
        Some(RejectReason::MaxTradesPerDay)
    );
    // next day, same time
    let next = t(24 * 3_600_000);
    let mut i = inst();
    i.expiry = Some(next.ist_date());
    assert!(rejected(&r.check(&enter(1), &i, &b.view(), next)).is_none());
}

#[test]
fn rate_cap_per_second_and_per_minute() {
    let mut limits = RiskLimits::default();
    limits.max_trades_per_day = 1_000;
    limits.max_orders_per_min = 3;
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    let b = Book::flat();
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))).is_none());
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(10))).is_none());
    // third inside the same second: per-second cap of 2
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(999))),
        Some(RejectReason::RateLimited)
    );
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(1_000))).is_none());
    // per-minute cap of 3 reached
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(30_000))),
        Some(RejectReason::RateLimited)
    );
    // the first order leaves the window after 60 s
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(60_000))).is_none());
}

#[test]
fn entry_window_boundaries() {
    let b = Book::flat();
    let cases = [
        (at(9, 19), Some(RejectReason::OutsideEntryWindow)),
        (at(9, 20), None),
        (at(14, 59), None),
        (at(15, 0), Some(RejectReason::OutsideEntryWindow)),
    ];
    for (now, want) in cases {
        let mut i = inst();
        i.age_ms = Some(0);
        let mut r = engine();
        assert_eq!(
            rejected(&r.check(&enter(1), &i, &b.view(), now)),
            want,
            "{now:?}"
        );
    }
}

#[test]
fn expiry_day_only() {
    let mut r = engine();
    let mut i = inst();
    i.expiry = Date::new(2026, 11, 10);
    assert_eq!(
        rejected(&r.check(&enter(1), &i, &Book::flat().view(), t(0))),
        Some(RejectReason::NotExpiryDay)
    );
    let mut limits = RiskLimits::default();
    limits.expiry_day_only = false;
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    assert!(rejected(&r.check(&enter(1), &i, &Book::flat().view(), t(0))).is_none());
}

#[test]
fn stale_data_guard_boundary() {
    let b = Book::flat();
    let mut i = inst();
    i.age_ms = Some(1_500);
    assert!(rejected(&engine().check(&enter(1), &i, &b.view(), t(0))).is_none());
    i.age_ms = Some(1_501);
    assert_eq!(
        rejected(&engine().check(&enter(1), &i, &b.view(), t(0))),
        Some(RejectReason::StaleData)
    );
    i.age_ms = None;
    assert_eq!(
        rejected(&engine().check(&enter(1), &i, &b.view(), t(0))),
        Some(RejectReason::StaleData)
    );
    let mut b = Book::flat();
    b.feed_stale = true;
    assert_eq!(
        rejected(&engine().check(&enter(1), &inst(), &b.view(), t(0))),
        Some(RejectReason::StaleData)
    );
}

#[test]
fn spread_guard_boundary() {
    let b = Book::flat();
    let mut i = inst();
    // bid 98.00, ask 100.00: spread 2.00 on mid 99.00 = 202 bps
    i.best_bid = Some(Paise(9_800));
    assert_eq!(
        rejected(&engine().check(&enter(1), &i, &b.view(), t(0))),
        Some(RejectReason::SpreadTooWide)
    );
    // bid 98.02, ask 100.00: 198 / 9901 = 199 bps
    i.best_bid = Some(Paise(9_802));
    assert!(rejected(&engine().check(&enter(1), &i, &b.view(), t(0))).is_none());
    i.best_bid = None;
    assert_eq!(
        rejected(&engine().check(&enter(1), &i, &b.view(), t(0))),
        Some(RejectReason::NoDepth)
    );
}

#[test]
fn consecutive_loss_pause() {
    let mut r = engine();
    let b = Book::flat();
    r.on_trade_closed(Paise(-100), t(0));
    r.on_trade_closed(Paise(-100), t(0));
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(0))).is_none());
    r.on_trade_closed(Paise(-100), t(1_000));
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(2_000))),
        Some(RejectReason::LossStreakPause)
    );
    // 15 minutes after the third loss the pause ends
    assert_eq!(
        rejected(&r.check(&enter(1), &inst(), &b.view(), t(1_000 + 15 * 60_000 - 1))),
        Some(RejectReason::LossStreakPause)
    );
    assert!(rejected(&r.check(&enter(1), &inst(), &b.view(), t(1_000 + 15 * 60_000))).is_none());
}

#[test]
fn a_win_resets_the_loss_streak() {
    let mut r = engine();
    r.on_trade_closed(Paise(-100), t(0));
    r.on_trade_closed(Paise(-100), t(0));
    r.on_trade_closed(Paise(1), t(0));
    r.on_trade_closed(Paise(-100), t(0));
    assert!(rejected(&r.check(&enter(1), &inst(), &Book::flat().view(), t(0))).is_none());
}

#[test]
fn margin_check_boundary() {
    let mut b = Book::flat();
    b.funds = Paise(650_650); // exactly one lot at 100.10
    assert!(rejected(&engine().check(&enter(1), &inst(), &b.view(), t(0))).is_none());
    b.funds = Paise(650_649);
    assert_eq!(
        rejected(&engine().check(&enter(1), &inst(), &b.view(), t(0))),
        Some(RejectReason::InsufficientMargin)
    );
    let mut limits = RiskLimits::default();
    limits.per_trade_max_lots = 5;
    limits.per_trade_max_premium = Paise::from_rupees(100_000);
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    b.funds = Paise(650_650 * 2 + 10);
    let d = r.check(&enter(5), &inst(), &b.view(), t(0));
    let RiskDecision::Resize(o, why) = d else {
        panic!("{d:?}")
    };
    assert_eq!((o.lots(), why), (2, RejectReason::ResizedToMargin));
}

#[test]
fn short_entries_and_zero_qty_rejected() {
    let mut s = enter(1);
    s.side = Side::Sell;
    assert_eq!(
        rejected(&engine().check(&s, &inst(), &Book::flat().view(), t(0))),
        Some(RejectReason::ShortEntryNotAllowed)
    );
    assert_eq!(
        rejected(&engine().check(&enter(0), &inst(), &Book::flat().view(), t(0))),
        Some(RejectReason::ZeroQuantity)
    );
}

#[test]
fn no_signal_sequence_exceeds_limits() {
    // A simple deterministic fuzz: random entries and exits, random books,
    // over a day. The engine must never approve more than the caps.
    let mut limits = RiskLimits::default();
    limits.max_trades_per_day = 7;
    let mut r = RiskEngine::new(limits, MarketHours::default()).unwrap();
    let mut seed: u64 = 12345;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut entries = 0;
    let mut sent: Vec<Ts> = vec![];
    for step in 0..5_000 {
        let now = at(9, 20).plus_ms(step * 300);
        let lots = (rnd() % 5) as u32;
        let s = if rnd() % 3 == 0 { exit() } else { enter(lots) };
        let b = if rnd() % 2 == 0 {
            Book::long(TOKEN, 65)
        } else {
            Book::flat()
        };
        let d = r.check(&s, &inst(), &b.view(), now);
        if let Some(o) = d.order() {
            assert!(o.lots() <= 1, "lots cap");
            if !o.is_exit() {
                entries += 1;
                let premium = o.limit().unwrap().times(o.qty() as i64);
                assert!(premium <= limits.per_trade_max_premium);
                assert!(now.ist_minute_of_day() < limits.entry_end.0);
                let last_sec = sent.iter().filter(|t| now.ms_since(**t) < 1_000).count();
                assert!(last_sec < 2);
            }
            sent.push(now);
        }
    }
    assert!(entries <= 7);
}
