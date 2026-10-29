use crate::kill::{KillCause, KillState};
use crate::limits::{LimitsError, MarketHours, RiskLimits};
use crate::rate::RateWindow;
use crate::types::*;
use es_core::{Date, Intent, OrderType, Paise, ReasonCode, Side, Signal, Ts};
use serde::Serialize;
use std::collections::VecDeque;
use tracing::{info, warn};

/// Something the engine must do now, raised by [`RiskEngine::monitor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskAction {
    /// The kill switch just fired: cancel working entries, disable
    /// strategies, flatten everything.
    KillSwitch(KillCause),
    /// Positions must be closed (after the flatten time, or while the kill
    /// switch is on and something is still open).
    Flatten,
}

/// Usage against limits, for the client API.
#[derive(Debug, Clone, Serialize)]
pub struct RiskUsage {
    pub pnl: Paise,
    pub loss_used: Paise,
    pub trades_today: u32,
    pub open_positions: u32,
    pub orders_last_sec: u32,
    pub orders_last_min: u32,
    pub loss_streak: u32,
    pub paused_until: Option<Ts>,
    pub kill: KillState,
    pub last_rejections: Vec<(Ts, u32, &'static str, RejectReason)>,
}

const REJECTION_LOG: usize = 20;

pub struct RiskEngine {
    limits: RiskLimits,
    hours: MarketHours,
    kill: KillState,
    day: Option<Date>,
    trades_today: u32,
    loss_streak: u32,
    paused_until: Option<Ts>,
    rate: RateWindow,
    rejections: VecDeque<(Ts, u32, &'static str, RejectReason)>,
}

impl RiskEngine {
    pub fn new(limits: RiskLimits, hours: MarketHours) -> Result<RiskEngine, LimitsError> {
        limits.validate()?;
        Ok(RiskEngine {
            rate: RateWindow::new(limits.max_orders_per_min),
            limits,
            hours,
            kill: KillState::Armed,
            day: None,
            trades_today: 0,
            loss_streak: 0,
            paused_until: None,
            rejections: VecDeque::with_capacity(REJECTION_LOG),
        })
    }

    pub fn limits(&self) -> &RiskLimits {
        &self.limits
    }

    pub fn kill_state(&self) -> KillState {
        self.kill
    }

    fn roll_day(&mut self, now: Ts) {
        let today = now.ist_date();
        if self.day != Some(today) {
            if self.day.is_some() {
                info!(%today, "risk: new trading day, daily counters reset");
            }
            self.day = Some(today);
            self.trades_today = 0;
            self.loss_streak = 0;
            self.paused_until = None;
        }
    }

    fn reject(&mut self, now: Ts, s: &Signal, r: RejectReason) -> RiskDecision {
        info!(strategy = s.strategy, token = s.instrument, reason = ?r, "risk: reject");
        if self.rejections.len() == REJECTION_LOG {
            self.rejections.pop_front();
        }
        self.rejections
            .push_back((now, s.instrument, s.strategy, r));
        RiskDecision::Reject(r)
    }

    fn in_market_hours(&self, now: Ts) -> bool {
        let m = now.ist_minute_of_day();
        m >= self.hours.open.0 && m < self.hours.close.0
    }

    /// Checks a signal. Every order in the system comes from here.
    pub fn check(
        &mut self,
        s: &Signal,
        inst: &InstrumentView,
        book: &PortfolioView,
        now: Ts,
    ) -> RiskDecision {
        self.roll_day(now);
        if inst.token != s.instrument || inst.lot_size == 0 {
            return self.reject(now, s, RejectReason::UnknownInstrument);
        }
        if s.intent.is_exit() {
            return self.check_exit(s, inst, book, now);
        }
        self.check_entry(s, inst, book, now)
    }

    fn check_exit(
        &mut self,
        s: &Signal,
        inst: &InstrumentView,
        book: &PortfolioView,
        now: Ts,
    ) -> RiskDecision {
        // Exits are never blocked by entry rules, the kill switch or the rate cap.
        let Some(pos) = book.position(s.instrument).copied() else {
            return self.reject(now, s, RejectReason::NoPosition);
        };
        let held = pos.qty.unsigned_abs() as u32;
        let target_units = match s.intent {
            Intent::ReduceTo(lots) => (lots * inst.lot_size).min(held),
            _ => 0,
        };
        let to_close = held
            .saturating_sub(target_units)
            .saturating_sub(pos.pending_exit_qty);
        let lots = to_close / inst.lot_size;
        if lots == 0 {
            return self.reject(now, s, RejectReason::ExitAlreadyPending);
        }
        let side = if pos.qty > 0 { Side::Sell } else { Side::Buy };
        let (order_type, limit) = match (s.limit, self.exit_price(side, inst)) {
            (Some(l), _) if l.0 > 0 => (OrderType::Limit, Some(l)),
            (_, Some(p)) => (OrderType::Limit, Some(p)),
            // No opposite quote: market order with protection.
            (_, None) => (OrderType::Market, None),
        };
        self.rate.record(now);
        let req = OrderRequest::new(
            s.instrument,
            side,
            lots,
            inst.lot_size,
            order_type,
            limit,
            self.limits.market_protection_pct,
            true,
            s.strategy,
            s.reason,
            now,
        );
        info!(
            strategy = s.strategy,
            token = s.instrument,
            lots,
            ?side,
            "risk: exit approved"
        );
        RiskDecision::Approve(req)
    }

    /// A market exit with protection, used when a kill switch exit at a
    /// limit price did not fill in time.
    pub fn check_market_exit(
        &mut self,
        strategy: &'static str,
        inst: &InstrumentView,
        book: &PortfolioView,
        now: Ts,
    ) -> RiskDecision {
        let s = Signal {
            strategy,
            instrument: inst.token,
            side: Side::Sell,
            intent: Intent::Exit,
            qty_lots: 0,
            limit: None,
            confidence: 1.0,
            reason: ReasonCode::KillSwitch,
        };
        let mut view = *inst;
        view.best_bid = None;
        view.best_ask = None;
        self.check(&s, &view, book, now)
    }

    fn exit_price(&self, side: Side, inst: &InstrumentView) -> Option<Paise> {
        let buffer = inst.tick_size.0.max(1) * self.limits.limit_buffer_ticks as i64;
        match side {
            Side::Sell => inst
                .best_bid
                .map(|b| Paise((b.0 - buffer).max(inst.tick_size.0.max(1)))),
            Side::Buy => inst.best_ask.map(|a| Paise(a.0 + buffer)),
        }
    }

    fn check_entry(
        &mut self,
        s: &Signal,
        inst: &InstrumentView,
        book: &PortfolioView,
        now: Ts,
    ) -> RiskDecision {
        use RejectReason as R;
        let l = self.limits;
        if self.kill.is_triggered() {
            return self.reject(now, s, R::KillSwitch);
        }
        if book.total_pnl() <= -l.daily_max_loss {
            self.trigger_kill(KillCause::DailyMaxLoss, now);
            return self.reject(now, s, R::DailyMaxLoss);
        }
        if s.qty_lots == 0 {
            return self.reject(now, s, R::ZeroQuantity);
        }
        if s.side != Side::Buy {
            return self.reject(now, s, R::ShortEntryNotAllowed);
        }
        let minute = now.ist_minute_of_day();
        if minute < l.entry_start.0 || minute >= l.entry_end.0 {
            return self.reject(now, s, R::OutsideEntryWindow);
        }
        if l.expiry_day_only && inst.expiry != Some(now.ist_date()) {
            return self.reject(now, s, R::NotExpiryDay);
        }
        match inst.age_ms {
            Some(age) if age <= l.stale_after_ms as i64 && !book.feed_stale => {}
            _ => return self.reject(now, s, R::StaleData),
        }
        let Some(ask) = inst.best_ask.filter(|p| p.0 > 0) else {
            return self.reject(now, s, R::NoDepth);
        };
        if inst.best_bid.is_none() {
            return self.reject(now, s, R::NoDepth);
        }
        match inst.spread_bps() {
            Some(bps) if bps <= l.max_spread_bps => {}
            _ => return self.reject(now, s, R::SpreadTooWide),
        }
        if let Some(until) = self.paused_until {
            if now < until {
                return self.reject(now, s, R::LossStreakPause);
            }
            self.paused_until = None;
        }
        if self.trades_today >= l.max_trades_per_day {
            return self.reject(now, s, R::MaxTradesPerDay);
        }
        let adds_position = book.position(s.instrument).is_none()
            && !book.pending_entry_tokens.contains(&s.instrument);
        if adds_position && book.open_count() >= l.max_open_positions {
            return self.reject(now, s, R::MaxOpenPositions);
        }
        if !self
            .rate
            .allows(now, l.max_orders_per_sec, l.max_orders_per_min)
        {
            return self.reject(now, s, R::RateLimited);
        }
        let buffer = inst.tick_size.0.max(1) * l.limit_buffer_ticks as i64;
        let price = match s.limit {
            Some(p) if p.0 > 0 => p,
            _ => Paise(ask.0 + buffer),
        };
        let lot_cost = price.times(inst.lot_size as i64);
        let mut lots = s.qty_lots;
        let mut resized = None;
        if lots > l.per_trade_max_lots {
            lots = l.per_trade_max_lots;
            resized = Some(R::ResizedToMaxLots);
        }
        let max_by_premium = (l.per_trade_max_premium.0 / lot_cost.0.max(1)) as u32;
        if max_by_premium == 0 {
            return self.reject(now, s, R::PremiumTooHigh);
        }
        if lots > max_by_premium {
            lots = max_by_premium;
            resized = Some(R::ResizedToMaxPremium);
        }
        if l.margin_check {
            let max_by_funds = (book.available_funds.0.max(0) / lot_cost.0.max(1)) as u32;
            if max_by_funds == 0 {
                return self.reject(now, s, R::InsufficientMargin);
            }
            if lots > max_by_funds {
                lots = max_by_funds;
                resized = Some(R::ResizedToMargin);
            }
        }
        self.rate.record(now);
        self.trades_today += 1;
        let req = OrderRequest::new(
            s.instrument,
            Side::Buy,
            lots,
            inst.lot_size,
            OrderType::Limit,
            Some(price),
            l.market_protection_pct,
            false,
            s.strategy,
            s.reason,
            now,
        );
        info!(
            strategy = s.strategy,
            token = s.instrument,
            lots,
            price = price.0,
            "risk: entry approved"
        );
        match resized {
            Some(r) => RiskDecision::Resize(req, r),
            None => RiskDecision::Approve(req),
        }
    }

    /// Checks a change to a working order. Exits may always be modified.
    /// Entries are checked against the per-trade size and premium limits and
    /// the kill switch.
    pub fn check_modify(
        &mut self,
        order: &OpenOrderView,
        new_lots: u32,
        new_limit: Paise,
        now: Ts,
    ) -> Result<ApprovedModify, RejectReason> {
        self.roll_day(now);
        if new_lots == 0 || order.lot_size == 0 {
            return Err(RejectReason::ZeroQuantity);
        }
        if new_limit.0 <= 0 {
            return Err(RejectReason::NoDepth);
        }
        if !order.is_exit {
            if self.kill.is_triggered() {
                return Err(RejectReason::KillSwitch);
            }
            if new_lots > self.limits.per_trade_max_lots {
                return Err(RejectReason::ResizedToMaxLots);
            }
            let premium = new_limit.times((new_lots * order.lot_size) as i64);
            if premium > self.limits.per_trade_max_premium {
                return Err(RejectReason::PremiumTooHigh);
            }
        }
        Ok(ApprovedModify::new(new_lots * order.lot_size, new_limit))
    }

    /// Called on every mark update. Fires the kill switch on the daily loss
    /// rule and asks for flattening when needed.
    pub fn monitor(&mut self, book: &PortfolioView, now: Ts) -> Option<RiskAction> {
        self.roll_day(now);
        if !self.kill.is_triggered() && book.total_pnl() <= -self.limits.daily_max_loss {
            self.trigger_kill(KillCause::DailyMaxLoss, now);
            return Some(RiskAction::KillSwitch(KillCause::DailyMaxLoss));
        }
        let after_flatten = now.ist_minute_of_day() >= self.limits.flatten_at.0;
        if (after_flatten || self.kill.is_triggered()) && !book.is_flat() {
            return Some(RiskAction::Flatten);
        }
        None
    }

    /// Fires the kill switch. Idempotent.
    pub fn trigger_kill(&mut self, cause: KillCause, now: Ts) -> bool {
        if self.kill.is_triggered() {
            return false;
        }
        warn!(?cause, "risk: KILL SWITCH triggered");
        self.kill = KillState::Triggered {
            cause,
            at: now,
            day: now.ist_date(),
        };
        true
    }

    /// Re-arms the kill switch. Requires the book to be flat. A daily loss
    /// trigger can only be re-armed on a later day.
    pub fn rearm(&mut self, book: &PortfolioView, now: Ts) -> Result<(), &'static str> {
        match self.kill {
            KillState::Armed => Ok(()),
            KillState::Triggered { cause, day, .. } => {
                if !book.is_flat() {
                    return Err("Close all positions before re-arming.");
                }
                if cause == KillCause::DailyMaxLoss && day == now.ist_date() {
                    return Err("Daily max loss was hit today. Entries resume tomorrow.");
                }
                info!("risk: kill switch re-armed");
                self.kill = KillState::Armed;
                Ok(())
            }
        }
    }

    /// Records a closed round trip for the consecutive loss rule.
    pub fn on_trade_closed(&mut self, pnl: Paise, now: Ts) {
        self.roll_day(now);
        if pnl < Paise::ZERO {
            self.loss_streak += 1;
            if self.loss_streak >= self.limits.loss_streak_pause {
                let until = now.plus_ms(self.limits.loss_pause_minutes as i64 * 60_000);
                info!(streak = self.loss_streak, "risk: loss streak pause");
                self.paused_until = Some(until);
                self.loss_streak = 0;
            }
        } else {
            self.loss_streak = 0;
        }
    }

    /// Replaces the limits. During market hours a loosening change needs a
    /// flat book and explicit confirmation.
    pub fn set_limits(
        &mut self,
        new: RiskLimits,
        book: &PortfolioView,
        confirmed: bool,
        now: Ts,
    ) -> Result<(), LimitsError> {
        new.validate()?;
        let looser = !self.limits.is_tightened_by(&new);
        if looser && self.in_market_hours(now) && !(book.is_flat() && confirmed) {
            return Err(LimitsError::LooserNeedsFlatAndConfirm);
        }
        info!(?new, "risk: limits changed");
        self.rate.reserve(new.max_orders_per_min);
        self.limits = new;
        Ok(())
    }

    pub fn usage(&mut self, book: &PortfolioView, now: Ts) -> RiskUsage {
        self.roll_day(now);
        let (s, m) = self.rate.counts(now);
        let pnl = book.total_pnl();
        RiskUsage {
            pnl,
            loss_used: Paise((-pnl.0).max(0)),
            trades_today: self.trades_today,
            open_positions: book.open_count(),
            orders_last_sec: s,
            orders_last_min: m,
            loss_streak: self.loss_streak,
            paused_until: self.paused_until.filter(|u| *u > now),
            kill: self.kill,
            last_rejections: self.rejections.iter().copied().collect(),
        }
    }
}
