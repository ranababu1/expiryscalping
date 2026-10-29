use super::*;

#[test]
fn defaults_match_the_plan_and_validate() {
    let l = RiskLimits::default();
    assert_eq!(l.daily_max_loss, Paise::from_rupees(2_000));
    assert_eq!(l.per_trade_max_premium, Paise::from_rupees(15_000));
    assert_eq!(l.per_trade_max_lots, 1);
    assert_eq!(l.max_open_positions, 2);
    assert_eq!(l.max_trades_per_day, 20);
    assert_eq!((l.max_orders_per_sec, l.max_orders_per_min), (2, 60));
    assert_eq!(l.entry_start.to_string(), "09:20");
    assert_eq!(l.entry_end.to_string(), "15:00");
    assert_eq!(l.flatten_at.to_string(), "15:15");
    assert!(l.expiry_day_only && l.margin_check);
    assert_eq!(l.stale_after_ms, 1_500);
    assert_eq!(l.max_spread_bps, 200);
    assert_eq!((l.loss_streak_pause, l.loss_pause_minutes), (3, 15));
    assert!(l.validate().is_ok());
}

#[test]
fn validation_errors() {
    let mut l = RiskLimits::default();
    l.daily_max_loss = Paise::ZERO;
    assert_eq!(
        l.validate(),
        Err(LimitsError::MustBePositive("daily_max_loss"))
    );
    let mut l = RiskLimits::default();
    l.entry_end = l.entry_start;
    assert_eq!(l.validate(), Err(LimitsError::BadWindow));
    let mut l = RiskLimits::default();
    l.max_orders_per_sec = 11;
    l.max_orders_per_min = 100;
    assert_eq!(l.validate(), Err(LimitsError::BadRate));
    assert!(RiskEngine::new(l, MarketHours::default()).is_err());
}

#[test]
fn tighter_changes_allowed_in_market_hours() {
    let mut r = engine();
    let mut l = RiskLimits::default();
    l.daily_max_loss = Paise::from_rupees(1_000);
    let b = Book::long(TOKEN, 65);
    assert!(r.set_limits(l, &b.view(), false, t(0)).is_ok());
    assert_eq!(r.limits().daily_max_loss, Paise::from_rupees(1_000));
}

#[test]
fn looser_changes_in_market_hours_need_flat_and_confirm() {
    let mut r = engine();
    let mut l = RiskLimits::default();
    l.daily_max_loss = Paise::from_rupees(3_000);
    let open = Book::long(TOKEN, 65);
    assert_eq!(
        r.set_limits(l, &open.view(), true, t(0)),
        Err(LimitsError::LooserNeedsFlatAndConfirm)
    );
    assert_eq!(
        r.set_limits(l, &Book::flat().view(), false, t(0)),
        Err(LimitsError::LooserNeedsFlatAndConfirm)
    );
    assert!(r.set_limits(l, &Book::flat().view(), true, t(0)).is_ok());
    // outside market hours, loosening is fine
    let mut r = engine();
    assert!(r.set_limits(l, &open.view(), false, at(8, 0)).is_ok());
}

#[test]
fn hhmm_parse_and_serde() {
    assert_eq!(HhMm::parse("09:20"), Some(HhMm(560)));
    assert_eq!(HhMm::parse("24:00"), None);
    assert_eq!(HhMm::parse("9"), None);
    let l = RiskLimits::default();
    let json = serde_json::to_string(&l).unwrap();
    assert!(json.contains("\"entry_start\":\"09:20\""), "{json}");
    let back: RiskLimits = serde_json::from_str(&json).unwrap();
    assert_eq!(back, l);
    let bad = json.replace("09:20", "9.20");
    assert!(serde_json::from_str::<RiskLimits>(&bad).is_err());
}

#[test]
fn usage_reports() {
    let mut r = engine();
    let mut b = Book::long(TOKEN, 65);
    b.realised = Paise(-50_000);
    let _ = r.check(&enter(1), &inst(), &Book::flat().view(), t(0));
    let mut i = inst();
    i.age_ms = None;
    let _ = r.check(&enter(1), &i, &Book::flat().view(), t(10));
    let u = r.usage(&b.view(), t(20));
    assert_eq!(u.loss_used, Paise(50_000));
    assert_eq!(u.trades_today, 1);
    assert_eq!(u.open_positions, 1);
    assert_eq!(u.orders_last_min, 1);
    assert_eq!(u.last_rejections.len(), 1);
    assert_eq!(u.last_rejections[0].3, RejectReason::StaleData);
}
