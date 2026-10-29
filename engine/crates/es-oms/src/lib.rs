//! Order manager. Owns every order's lifecycle as a state machine:
//!
//! ```text
//! Created -> Sent -> Open -> (PartiallyFilled) -> Complete
//!                  \-> Rejected
//!           Open -> ModifyPending -> Open
//!           Open -> CancelPending -> Cancelled
//!           Sent -> Unknown (timeout or network error) -> reconciled
//! ```
//!
//! The order manager is synchronous and does no IO. The engine calls the
//! broker and reports results back (`on_place_result`, `on_event`, ...),
//! which keeps the state machine deterministic and easy to test.

mod limits;
mod portfolio;

pub use limits::{CallCounter, KiteLimits};
pub use portfolio::Position;

use es_broker::charges::ChargesConfig;
use es_broker::{BrokerError, BrokerEvent, BrokerOrder, BrokerOrderId, BrokerOrderStatus};
use es_core::{Fill, OrderId, OrderType, Paise, ReasonCode, Side, Ts};
use es_risk::{OpenOrderView, OrderRequest, PortfolioView, PositionView};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OrderState {
    Created,
    Sent,
    Open,
    PartiallyFilled,
    ModifyPending,
    CancelPending,
    Complete,
    Rejected,
    Cancelled,
    Unknown,
}

impl OrderState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OrderState::Complete | OrderState::Rejected | OrderState::Cancelled
        )
    }

    /// Working at the broker (can fill, be modified or cancelled).
    pub fn is_working(self) -> bool {
        matches!(
            self,
            OrderState::Open
                | OrderState::PartiallyFilled
                | OrderState::ModifyPending
                | OrderState::CancelPending
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OmsError {
    #[error("unknown order")]
    UnknownOrder,
    #[error("Kite order rate limit reached, retry shortly")]
    RateLimited,
    #[error("order is {0:?}, not open")]
    NotOpen(OrderState),
    #[error("order reached {0} modifications, cancel and re-place")]
    ModifyLimit(u32),
}

/// An order and its history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedOrder {
    pub id: OrderId,
    pub tag: String,
    pub broker_id: Option<BrokerOrderId>,
    pub token: u32,
    pub exchange: String,
    pub lot_size: u32,
    pub side: Side,
    pub qty: u32,
    pub filled: u32,
    pub avg_fill_price: Paise,
    pub limit: Option<Paise>,
    pub order_type: OrderType,
    pub is_exit: bool,
    pub strategy: &'static str,
    pub reason: ReasonCode,
    pub state: OrderState,
    pub modifications: u32,
    pub message: Option<String>,
    pub created_at: Ts,
    pub sent_at: Option<Ts>,
    pub updated_at: Ts,
    /// State to return to if a modify or cancel fails.
    #[serde(skip)]
    before_pending: Option<OrderState>,
    /// The broker said complete before we saw every fill.
    #[serde(skip)]
    complete_seen: bool,
    #[serde(skip)]
    charged_brokerage: bool,
}

impl ManagedOrder {
    pub fn remaining(&self) -> u32 {
        self.qty.saturating_sub(self.filled)
    }

    fn open_state(&self) -> OrderState {
        if self.filled > 0 {
            OrderState::PartiallyFilled
        } else {
            OrderState::Open
        }
    }
}

/// Something the engine should pass on (to strategies, risk, the API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmsUpdate {
    Order(OrderId),
    Fill(Fill),
    /// A position went flat. `pnl` is net of charges, for the loss streak.
    TradeClosed {
        token: u32,
        pnl: Paise,
    },
}

pub struct Oms {
    limits: KiteLimits,
    charges: ChargesConfig,
    calls: CallCounter,
    next_id: u64,
    orders: BTreeMap<OrderId, ManagedOrder>,
    by_broker: HashMap<BrokerOrderId, OrderId>,
    trades_seen: HashSet<String>,
    positions: BTreeMap<u32, Position>,
}

impl Oms {
    pub fn new(limits: KiteLimits, charges: ChargesConfig) -> Oms {
        Oms {
            limits,
            charges,
            calls: CallCounter::default(),
            next_id: 1,
            orders: BTreeMap::new(),
            by_broker: HashMap::new(),
            trades_seen: HashSet::new(),
            positions: BTreeMap::new(),
        }
    }

    pub fn order(&self, id: OrderId) -> Option<&ManagedOrder> {
        self.orders.get(&id)
    }

    pub fn orders(&self) -> impl Iterator<Item = &ManagedOrder> {
        self.orders.values()
    }

    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values()
    }

    pub fn position(&self, token: u32) -> Option<&Position> {
        self.positions.get(&token)
    }

    pub fn calls_today(&self) -> u32 {
        self.calls.today()
    }

    /// Records a new risk-approved order. The engine then calls
    /// `mark_sent`, the broker, and `on_place_result`.
    pub fn create(
        &mut self,
        req: OrderRequest,
        exchange: &str,
        lot_size: u32,
        now: Ts,
    ) -> Result<OrderId, OmsError> {
        if !self.calls.allows(&self.limits, now) {
            return Err(OmsError::RateLimited);
        }
        let id = OrderId(self.next_id);
        self.next_id += 1;
        let tag = make_tag(req.strategy(), id.0);
        let o = ManagedOrder {
            id,
            tag,
            broker_id: None,
            token: req.token(),
            exchange: exchange.to_string(),
            lot_size,
            side: req.side(),
            qty: req.qty(),
            filled: 0,
            avg_fill_price: Paise::ZERO,
            limit: req.limit(),
            order_type: req.order_type(),
            is_exit: req.is_exit(),
            strategy: req.strategy(),
            reason: req.reason(),
            state: OrderState::Created,
            modifications: 0,
            message: None,
            created_at: now,
            sent_at: None,
            updated_at: now,
            before_pending: None,
            complete_seen: false,
            charged_brokerage: false,
        };
        self.orders.insert(id, o);
        Ok(id)
    }

    pub fn mark_sent(&mut self, id: OrderId, now: Ts) -> Result<(), OmsError> {
        let o = self.orders.get_mut(&id).ok_or(OmsError::UnknownOrder)?;
        if o.state != OrderState::Created {
            return Err(OmsError::NotOpen(o.state));
        }
        o.state = OrderState::Sent;
        o.sent_at = Some(now);
        o.updated_at = now;
        self.calls.record(now);
        Ok(())
    }

    pub fn on_place_result(
        &mut self,
        id: OrderId,
        result: Result<BrokerOrderId, BrokerError>,
        now: Ts,
    ) -> Vec<OmsUpdate> {
        let Some(o) = self.orders.get_mut(&id) else {
            return Vec::new();
        };
        if o.state != OrderState::Sent {
            debug!(?id, state = ?o.state, "place result for an order not in Sent");
            return Vec::new();
        }
        o.updated_at = now;
        match result {
            Ok(bid) => {
                o.state = OrderState::Open;
                o.broker_id = Some(bid.clone());
                self.by_broker.insert(bid, id);
            }
            Err(BrokerError::Unknown(m)) => {
                // Never retry blindly: reconcile from the order book first.
                warn!(?id, %m, "order state unknown");
                o.state = OrderState::Unknown;
                o.message = Some(m);
            }
            Err(e) => {
                o.state = OrderState::Rejected;
                o.message = Some(e.to_string());
            }
        }
        // Events may have arrived for a broker id before the place call
        // returned; the engine replays any it buffered through `on_event`.
        vec![OmsUpdate::Order(id)]
    }

    /// Starts a modification. Counts toward Kite's 25 per order.
    pub fn begin_modify(&mut self, id: OrderId, now: Ts) -> Result<BrokerOrderId, OmsError> {
        let max = self.limits.max_modifications;
        let o = self.orders.get_mut(&id).ok_or(OmsError::UnknownOrder)?;
        if !matches!(o.state, OrderState::Open | OrderState::PartiallyFilled) {
            return Err(OmsError::NotOpen(o.state));
        }
        if o.modifications >= max {
            return Err(OmsError::ModifyLimit(max));
        }
        let bid = o.broker_id.clone().ok_or(OmsError::NotOpen(o.state))?;
        if !self.calls.allows(&self.limits, now) {
            return Err(OmsError::RateLimited);
        }
        o.before_pending = Some(o.state);
        o.state = OrderState::ModifyPending;
        o.modifications += 1;
        o.updated_at = now;
        self.calls.record(now);
        Ok(bid)
    }

    pub fn on_modify_result(&mut self, id: OrderId, result: Result<(), BrokerError>, now: Ts) {
        let Some(o) = self.orders.get_mut(&id) else {
            return;
        };
        if o.state != OrderState::ModifyPending {
            return;
        }
        if let Err(e) = result {
            o.state = o.before_pending.take().unwrap_or(OrderState::Open);
            o.message = Some(e.to_string());
            o.updated_at = now;
        }
        // On success, wait for the broker's Modified event.
    }

    /// Starts a cancel. Cancels are never rate limited by us: they reduce
    /// risk. They still count toward the day.
    pub fn begin_cancel(&mut self, id: OrderId, now: Ts) -> Result<BrokerOrderId, OmsError> {
        let o = self.orders.get_mut(&id).ok_or(OmsError::UnknownOrder)?;
        if !matches!(
            o.state,
            OrderState::Open | OrderState::PartiallyFilled | OrderState::ModifyPending
        ) {
            return Err(OmsError::NotOpen(o.state));
        }
        let bid = o.broker_id.clone().ok_or(OmsError::NotOpen(o.state))?;
        if o.state != OrderState::ModifyPending {
            o.before_pending = Some(o.state);
        }
        o.state = OrderState::CancelPending;
        o.updated_at = now;
        self.calls.record(now);
        Ok(bid)
    }

    pub fn on_cancel_result(&mut self, id: OrderId, result: Result<(), BrokerError>, now: Ts) {
        let Some(o) = self.orders.get_mut(&id) else {
            return;
        };
        if o.state != OrderState::CancelPending {
            return;
        }
        match result {
            Ok(()) => {}
            // Already filled or cancelled: the event decides.
            Err(BrokerError::NotOpen) => {}
            Err(e) => {
                o.state = o.before_pending.take().unwrap_or(o.open_state());
                o.message = Some(e.to_string());
                o.updated_at = now;
            }
        }
    }

    /// Applies a broker postback. Handles duplicates (same trade id),
    /// out-of-order delivery (complete before the last fill, fills after a
    /// cancel) and events for orders we do not know (ignored).
    pub fn on_event(&mut self, ev: &BrokerEvent, now: Ts) -> Vec<OmsUpdate> {
        let bid = match ev {
            BrokerEvent::Fill { id, .. }
            | BrokerEvent::Complete { id }
            | BrokerEvent::Cancelled { id }
            | BrokerEvent::Rejected { id, .. }
            | BrokerEvent::Modified { id, .. } => id,
        };
        let Some(&id) = self.by_broker.get(bid) else {
            debug!(broker_id = %bid.0, "event for an order we do not manage");
            return Vec::new();
        };
        let mut out = Vec::new();
        match ev {
            BrokerEvent::Fill {
                trade_id,
                qty,
                price,
                ts,
                ..
            } => {
                if !self.trades_seen.insert(trade_id.clone()) {
                    return out;
                }
                self.apply_fill(id, *qty, *price, *ts, now, &mut out);
            }
            BrokerEvent::Complete { .. } => {
                if let Some(o) = self.orders.get_mut(&id)
                    && !o.state.is_terminal()
                {
                    o.complete_seen = true;
                    if o.filled >= o.qty {
                        o.state = OrderState::Complete;
                    }
                    o.updated_at = now;
                    out.push(OmsUpdate::Order(id));
                }
            }
            BrokerEvent::Cancelled { .. } => {
                if let Some(o) = self.orders.get_mut(&id)
                    && !o.state.is_terminal()
                {
                    o.state = OrderState::Cancelled;
                    o.updated_at = now;
                    out.push(OmsUpdate::Order(id));
                }
            }
            BrokerEvent::Rejected { reason, .. } => {
                if let Some(o) = self.orders.get_mut(&id)
                    && !o.state.is_terminal()
                {
                    o.state = OrderState::Rejected;
                    o.message = Some(reason.clone());
                    o.updated_at = now;
                    out.push(OmsUpdate::Order(id));
                }
            }
            BrokerEvent::Modified { qty, limit, .. } => {
                if let Some(o) = self.orders.get_mut(&id)
                    && !o.state.is_terminal()
                {
                    o.qty = *qty;
                    o.limit = Some(*limit);
                    o.order_type = OrderType::Limit;
                    if o.state == OrderState::ModifyPending {
                        o.before_pending = None;
                        o.state = o.open_state();
                    }
                    if o.filled >= o.qty {
                        o.state = OrderState::Complete;
                    }
                    o.updated_at = now;
                    out.push(OmsUpdate::Order(id));
                }
            }
        }
        out
    }

    fn apply_fill(
        &mut self,
        id: OrderId,
        qty: u32,
        price: Paise,
        ts: Ts,
        now: Ts,
        out: &mut Vec<OmsUpdate>,
    ) {
        let Some(o) = self.orders.get_mut(&id) else {
            return;
        };
        // Fills are always booked, even late ones after a cancel: positions
        // must match the exchange.
        let prev = o.filled as i128 * o.avg_fill_price.0 as i128;
        o.filled += qty;
        o.avg_fill_price =
            Paise(((prev + qty as i128 * price.0 as i128) / o.filled.max(1) as i128) as i64);
        let first = !o.charged_brokerage;
        o.charged_brokerage = true;
        let turnover = price.times(qty as i64);
        let charges = self.charges.on_fill(o.side, turnover, &o.exchange, first);
        if !o.state.is_terminal() {
            if o.filled >= o.qty {
                o.state = OrderState::Complete;
            } else if matches!(
                o.state,
                OrderState::Open | OrderState::Sent | OrderState::Unknown
            ) {
                o.state = OrderState::PartiallyFilled;
            }
        }
        o.updated_at = now;
        let (token, side, strategy) = (o.token, o.side, o.strategy);
        let pos = self
            .positions
            .entry(token)
            .or_insert_with(|| Position::new(token));
        let closed = pos.apply(side, qty, price, charges);
        out.push(OmsUpdate::Fill(Fill {
            order: id,
            token,
            side,
            qty,
            price,
            ts,
            strategy: strategy.to_string(),
        }));
        out.push(OmsUpdate::Order(id));
        if let Some(pnl) = closed {
            out.push(OmsUpdate::TradeClosed { token, pnl });
        }
    }

    /// Orders stuck in `Sent` past the timeout become `Unknown`.
    pub fn check_timeouts(&mut self, now: Ts, sent_timeout_ms: i64) -> Vec<OrderId> {
        let mut v = Vec::new();
        for o in self.orders.values_mut() {
            if o.state == OrderState::Sent
                && o.sent_at
                    .is_some_and(|s| now.ms_since(s) >= sent_timeout_ms)
            {
                o.state = OrderState::Unknown;
                o.message = Some("no reply from broker, reconciling".into());
                o.updated_at = now;
                v.push(o.id);
            }
        }
        v
    }

    pub fn has_unknown(&self) -> bool {
        self.orders.values().any(|o| o.state == OrderState::Unknown)
    }

    /// Reconciles `Unknown` orders against the broker's order book, matched
    /// by tag. Orders not found never reached the broker and are rejected.
    /// Fills found only in the order book are booked at the order's limit
    /// (or average fill) price; the trade book is the better source and
    /// arrives with the Kite broker in Phase 3.
    pub fn reconcile(&mut self, book: &[BrokerOrder], now: Ts) -> Vec<OmsUpdate> {
        let unknown: Vec<OrderId> = self
            .orders
            .values()
            .filter(|o| o.state == OrderState::Unknown)
            .map(|o| o.id)
            .collect();
        let mut out = Vec::new();
        for id in unknown {
            let Some(o) = self.orders.get_mut(&id) else {
                continue;
            };
            match book.iter().find(|b| b.tag == o.tag) {
                None => {
                    o.state = OrderState::Rejected;
                    o.message = Some("not found at broker".into());
                    o.updated_at = now;
                    out.push(OmsUpdate::Order(id));
                }
                Some(b) => {
                    o.broker_id = Some(b.id.clone());
                    self.by_broker.insert(b.id.clone(), id);
                    let missing = b.filled.saturating_sub(o.filled);
                    let price = o.limit.unwrap_or(o.avg_fill_price);
                    o.state = OrderState::Open;
                    if missing > 0 {
                        self.apply_fill(id, missing, price, now, now, &mut out);
                    }
                    if let Some(o) = self.orders.get_mut(&id) {
                        o.state = match b.status {
                            BrokerOrderStatus::Open => o.open_state(),
                            BrokerOrderStatus::Complete => OrderState::Complete,
                            BrokerOrderStatus::Cancelled => OrderState::Cancelled,
                            BrokerOrderStatus::Rejected => OrderState::Rejected,
                        };
                        o.updated_at = now;
                    }
                    out.push(OmsUpdate::Order(id));
                }
            }
        }
        out
    }

    /// The risk engine's view of a working order.
    pub fn open_order_view(&self, id: OrderId) -> Option<OpenOrderView> {
        self.orders.get(&id).map(|o| OpenOrderView {
            is_exit: o.is_exit,
            side: o.side,
            lot_size: o.lot_size,
        })
    }

    /// Working (or not yet acknowledged) order ids.
    pub fn working(&self) -> Vec<OrderId> {
        self.orders
            .values()
            .filter(|o| !o.state.is_terminal())
            .map(|o| o.id)
            .collect()
    }

    /// Snapshot for the risk engine. `mark` gives the current price per token.
    pub fn snapshot(&self, mark: impl Fn(u32) -> Option<Paise>) -> BookSnapshot {
        let mut positions = Vec::new();
        let mut realised = Paise::ZERO;
        let mut unrealised = Paise::ZERO;
        for p in self.positions.values() {
            realised += p.realised();
            let m = mark(p.token).unwrap_or(p.last_price);
            unrealised += p.unrealised(m);
            let pending_exit_qty = self
                .orders
                .values()
                .filter(|o| o.token == p.token && o.is_exit && !o.state.is_terminal())
                .map(|o| o.remaining())
                .sum();
            positions.push(PositionView {
                token: p.token,
                qty: p.qty,
                pending_exit_qty,
            });
        }
        let mut pending_entry_tokens: Vec<u32> = self
            .orders
            .values()
            .filter(|o| !o.is_exit && !o.state.is_terminal())
            .map(|o| o.token)
            .collect();
        pending_entry_tokens.sort_unstable();
        pending_entry_tokens.dedup();
        BookSnapshot {
            positions,
            pending_entry_tokens,
            realised,
            unrealised,
        }
    }
}

/// Owned book facts; `view` lends them to the risk engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookSnapshot {
    pub positions: Vec<PositionView>,
    pub pending_entry_tokens: Vec<u32>,
    pub realised: Paise,
    pub unrealised: Paise,
}

impl BookSnapshot {
    pub fn view(&self, available_funds: Paise, feed_stale: bool) -> PortfolioView<'_> {
        PortfolioView {
            realised: self.realised,
            unrealised: self.unrealised,
            positions: &self.positions,
            pending_entry_tokens: &self.pending_entry_tokens,
            available_funds,
            feed_stale,
        }
    }
}

/// `ES` + up to 4 alphanumeric strategy characters + sequence, at most 20.
pub fn make_tag(strategy: &str, seq: u64) -> String {
    let code: String = strategy
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase();
    let mut t = format!("ES{code}{seq}");
    t.truncate(20);
    t
}

#[cfg(test)]
mod tests;
