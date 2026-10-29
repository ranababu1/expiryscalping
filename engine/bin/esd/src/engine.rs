//! The engine loop. One task owns all trading state: ticks come in, market
//! state updates, the paper broker matches, strategies emit signals, the risk
//! engine approves or rejects, and the order manager tracks every order.
//!
//! Engine time is tick time (`RawRecord::recv`), so a replay of the same raw
//! log gives the same signals, orders and fills.

use crate::config::EngineConfig;
use crate::journal::Journal;
use es_api::{AlertDto, AlertLevel, FillDto, Publisher, ServerMsg, SideDto};
use es_broker::{Broker, BrokerEvent, PaperBroker};
use es_core::{Fill, Intent, Paise, ReasonCode, Side, Signal, Tick, Ts};
use es_feed::{FeedHealth, RawRecord, RecordKind};
use es_kite::ticker::{FrameKind, Parser};
use es_market::MarketState;
use es_oms::{Oms, OmsUpdate};
use es_risk::{
    InstrumentView, KillCause, OrderRequest, RiskAction, RiskDecision, RiskEngine, RiskLimits,
};
use es_strategy::{MomentumBreakout, Strategy};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{debug, info, warn};

pub const MANUAL: &str = "manual";

pub struct Slot {
    pub strategy: Box<dyn Strategy>,
    pub enabled: bool,
    pub description: &'static str,
}

/// A position the engine is trying to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitWant {
    pub strategy: &'static str,
    pub reason: ReasonCode,
    pub since: Ts,
}

/// Tick-to-decision latency samples, in microseconds.
#[derive(Default)]
pub struct Latency {
    samples: Vec<u32>,
    next: usize,
}

const LATENCY_WINDOW: usize = 4096;

impl Latency {
    pub fn record(&mut self, us: u32) {
        if self.samples.len() < LATENCY_WINDOW {
            self.samples.push(us);
        } else {
            self.samples[self.next] = us;
            self.next = (self.next + 1) % LATENCY_WINDOW;
        }
    }

    /// (p50, p99, count)
    pub fn summary(&self) -> (u32, u32, u32) {
        if self.samples.is_empty() {
            return (0, 0, 0);
        }
        let mut s = self.samples.clone();
        s.sort_unstable();
        let at = |q: usize| s[(s.len() - 1) * q / 100];
        (at(50), at(99), s.len() as u32)
    }
}

/// Where the engine's inputs come from, shown in the app.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub source: String,
    pub version: String,
    /// Folder for saved risk overrides and the audit log. `None` in tests.
    pub data_dir: Option<PathBuf>,
}

pub struct Engine {
    pub(crate) cfg: EngineConfig,
    pub(crate) opts: EngineOptions,
    pub(crate) market: Option<MarketState>,
    ticks: Vec<Tick>,
    pub(crate) feed: FeedHealth,
    pub(crate) risk: RiskEngine,
    pub(crate) oms: Oms,
    pub(crate) broker: PaperBroker,
    pub(crate) strategies: Vec<Slot>,
    pub(crate) ai_paused: bool,
    pub(crate) funds: Paise,
    pub(crate) exits: BTreeMap<u32, ExitWant>,
    last_timer: Option<Ts>,
    pub(crate) now: Ts,
    pub(crate) journal: Journal,
    pub(crate) publisher: Option<Publisher>,
    pub(crate) latency: Latency,
    pub(crate) chain_underlying: String,
    pub(crate) replay_done: bool,
    events: Vec<BrokerEvent>,
}

impl Engine {
    pub fn new(
        cfg: EngineConfig,
        limits: RiskLimits,
        opts: EngineOptions,
        journal: Journal,
        publisher: Option<Publisher>,
    ) -> Result<Engine, es_risk::LimitsError> {
        let risk = RiskEngine::new(limits, cfg.market.hours)?;
        let oms = Oms::new(cfg.kite_limits, cfg.charges);
        let broker = PaperBroker::new(cfg.paper);
        let m = &cfg.strategies.momentum;
        let strategies = vec![Slot {
            strategy: Box::new(MomentumBreakout::new(m.params.clone())),
            enabled: m.enabled,
            description: "Momentum breakout: buys the ATM call or put when the index moves \
                          sharply, exits on target, stop or time.",
        }];
        let chain_underlying = cfg
            .market
            .underlyings
            .first()
            .map(|u| u.name.clone())
            .unwrap_or_default();
        Ok(Engine {
            funds: cfg.paper.capital,
            cfg,
            opts,
            market: None,
            ticks: Vec::new(),
            feed: FeedHealth::default(),
            risk,
            oms,
            broker,
            strategies,
            ai_paused: false,
            exits: BTreeMap::new(),
            last_timer: None,
            now: Ts(0),
            journal,
            publisher,
            latency: Latency::default(),
            chain_underlying,
            replay_done: false,
            events: Vec::new(),
        })
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn now(&self) -> Ts {
        self.now
    }

    pub(crate) fn alert(&mut self, level: AlertLevel, code: &str, message: impl Into<String>) {
        let message = message.into();
        match level {
            AlertLevel::Error => warn!(code, %message, "alert"),
            _ => info!(code, %message, "alert"),
        }
        self.journal
            .write("alert", self.now.0, &(code, message.as_str()));
        if let Some(p) = &self.publisher {
            p.publish(ServerMsg::Alert(AlertDto {
                level,
                code: code.to_string(),
                message,
                ts: self.now.millis(),
            }));
        }
    }

    // ------------------------------------------------------------ inputs

    pub async fn on_record(&mut self, r: RawRecord) {
        if r.recv > self.now {
            self.now = r.recv;
        }
        match r.kind {
            RecordKind::Instruments => self.on_instruments(&r.bytes),
            RecordKind::Binary => self.on_binary(&r).await,
            RecordKind::Text => match std::str::from_utf8(&r.bytes)
                .ok()
                .map(es_kite::text::parse_text)
            {
                Some(Ok(m)) => debug!(?m, "feed text message"),
                _ => warn!("feed: unreadable text message"),
            },
        }
        self.maybe_timer().await;
    }

    fn on_instruments(&mut self, bytes: &[u8]) {
        let csv = String::from_utf8_lossy(bytes);
        match es_kite::instruments::parse_csv(&csv) {
            Ok(list) => {
                let n = list.len();
                self.market = Some(MarketState::new(list, &self.cfg.market.underlyings));
                self.feed.on_connect();
                info!(instruments = n, "instrument master loaded");
            }
            Err(e) => self.alert(
                AlertLevel::Error,
                "instruments",
                format!("Instrument master unreadable: {e}"),
            ),
        }
    }

    async fn on_binary(&mut self, r: &RawRecord) {
        self.feed.on_frame(r.recv);
        let mut ticks = std::mem::take(&mut self.ticks);
        ticks.clear();
        match Parser.parse_frame(&r.bytes, r.recv, &mut ticks) {
            Ok(FrameKind::Heartbeat) => {}
            Ok(FrameKind::Data { .. }) => {
                for t in &ticks {
                    self.on_tick(t).await;
                }
            }
            Err(e) => warn!(%e, "feed: bad frame"),
        }
        self.ticks = ticks;
    }

    async fn on_tick(&mut self, t: &Tick) {
        let started = Instant::now();
        let Some(market) = self.market.as_mut() else {
            return;
        };
        if !market.on_tick(t) {
            return;
        }
        self.broker.on_tick(t);
        self.pump().await;
        let mut sigs = Vec::new();
        if let Some(market) = self.market.as_ref() {
            for slot in &mut self.strategies {
                slot.strategy.on_tick(market, t, &mut sigs);
            }
        }
        for s in sigs {
            if let Err(e) = self.handle_signal(s).await {
                debug!(%e, "signal not sent");
            }
        }
        let us = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
        self.latency.record(us);
    }

    async fn maybe_timer(&mut self) {
        let due = match self.last_timer {
            None => true,
            Some(t) => self.now.ms_since(t) >= self.cfg.timing.timer_ms,
        };
        if due && self.market.is_some() {
            self.last_timer = Some(self.now);
            self.on_timer().await;
        }
    }

    // ------------------------------------------------------------ broker events

    /// Passes broker events to the order manager until there are none left.
    pub(crate) async fn pump(&mut self) {
        loop {
            let mut evs = std::mem::take(&mut self.events);
            evs.clear();
            self.broker.drain_events(&mut evs);
            if evs.is_empty() {
                self.events = evs;
                return;
            }
            let mut ups = Vec::new();
            for ev in &evs {
                ups.extend(self.oms.on_event(ev, self.now));
            }
            self.events = evs;
            self.handle_updates(ups).await;
        }
    }

    pub(crate) async fn handle_updates(&mut self, ups: Vec<OmsUpdate>) {
        let mut any_fill = false;
        for u in ups {
            match u {
                OmsUpdate::Fill(f) => {
                    any_fill = true;
                    self.on_fill(&f);
                }
                OmsUpdate::TradeClosed { token, pnl } => {
                    self.journal
                        .write("trade_closed", self.now.0, &(token, pnl));
                    self.risk.on_trade_closed(pnl, self.now);
                }
                OmsUpdate::Order(id) => self.on_order_update(id),
            }
        }
        if any_fill {
            match self.broker.margins().await {
                // The paper broker tracks premium cash only; charges are
                // booked by the order manager, so take them off here.
                Ok(m) => {
                    let charges: i64 = self.oms.positions().map(|p| p.charges.0).sum();
                    self.funds = m.available - Paise(charges);
                }
                Err(e) => warn!(%e, "margins unavailable"),
            }
        }
    }

    fn on_fill(&mut self, f: &Fill) {
        self.journal.write("fill", self.now.0, f);
        for slot in &mut self.strategies {
            // The owner always hears about its fills. Everyone hears about
            // sells, so a strategy notices when its position was closed for it.
            if slot.strategy.id() == f.strategy || f.side == Side::Sell {
                slot.strategy.on_fill(f);
            }
        }
        if let Some(p) = &self.publisher {
            let symbol = self.symbol(f.token);
            p.publish(ServerMsg::Fill(FillDto {
                order_id: f.order.0,
                token: f.token,
                symbol,
                side: side_dto(f.side),
                qty: f.qty,
                price: f.price.0,
                ts: f.ts.millis(),
                strategy: f.strategy.clone(),
            }));
        }
    }

    fn on_order_update(&mut self, id: es_core::OrderId) {
        let Some(o) = self.oms.order(id) else {
            return;
        };
        self.journal.write("order", self.now.0, o);
        if !o.state.is_terminal() {
            return;
        }
        let (token, strategy, is_exit, filled, msg) =
            (o.token, o.strategy, o.is_exit, o.filled, o.message.clone());
        if o.state == es_oms::OrderState::Rejected {
            self.alert(
                AlertLevel::Warn,
                "order_rejected",
                format!(
                    "Order {} for {} rejected: {}",
                    id.0,
                    self.symbol(token),
                    msg.unwrap_or_default()
                ),
            );
        }
        if !is_exit && filled == 0 {
            let now = self.now;
            if let Some(s) = self.slot_mut(strategy) {
                s.strategy.on_rejected(token, now);
            }
        }
    }

    // ------------------------------------------------------------ signals

    pub(crate) fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.strategies.iter_mut().find(|s| s.strategy.id() == id)
    }

    pub(crate) fn symbol(&self, token: u32) -> String {
        self.market
            .as_ref()
            .and_then(|m| m.instrument(token))
            .map(|i| i.tradingsymbol.clone())
            .unwrap_or_else(|| token.to_string())
    }

    pub(crate) fn instrument_view(&self, token: u32) -> Option<InstrumentView> {
        let m = self.market.as_ref()?;
        let i = m.instrument(token)?;
        let q = m.quote(token).copied().unwrap_or_default();
        Some(InstrumentView {
            token,
            lot_size: i.lot_size,
            tick_size: i.tick_size,
            expiry: i.expiry,
            best_bid: q.best_bid(),
            best_ask: q.best_ask(),
            ltp: q.ltp,
            age_ms: m.age_ms(token),
        })
    }

    pub(crate) fn mark(&self, token: u32) -> Option<Paise> {
        let q = self.market.as_ref()?.quote(token)?;
        let m = q.mark();
        (m.0 > 0).then_some(m)
    }

    pub(crate) fn book(&self) -> es_oms::BookSnapshot {
        self.oms.snapshot(|t| self.mark(t))
    }

    pub(crate) fn feed_stale(&self) -> bool {
        self.feed
            .is_stale(self.now, self.risk.limits().stale_after_ms as i64)
    }

    /// Runs a signal through risk and, if approved, sends the order.
    /// Returns the order id, or why nothing was sent.
    pub(crate) async fn handle_signal(&mut self, s: Signal) -> Result<es_core::OrderId, String> {
        self.journal.write("signal", self.now.0, &s);
        let manual = s.strategy == MANUAL;
        if !s.intent.is_exit() && !manual {
            let enabled = self
                .strategies
                .iter()
                .any(|x| x.strategy.id() == s.strategy && x.enabled);
            if !enabled || self.ai_paused {
                let now = self.now;
                if let Some(slot) = self.slot_mut(s.strategy) {
                    slot.strategy.on_rejected(s.instrument, now);
                }
                return Err("Strategy is disabled or paused.".into());
            }
        }
        let Some(inst) = self.instrument_view(s.instrument) else {
            return Err(format!("Unknown instrument {}.", s.instrument));
        };
        let book = self.book();
        let stale = self.feed_stale();
        let decision = self
            .risk
            .check(&s, &inst, &book.view(self.funds, stale), self.now);
        self.journal
            .write("decision", self.now.0, &decision_json(&decision));
        let req = match decision {
            RiskDecision::Approve(r) | RiskDecision::Resize(r, _) => r,
            RiskDecision::Reject(reason) => {
                let now = self.now;
                if !s.intent.is_exit()
                    && let Some(slot) = self.slot_mut(s.strategy)
                {
                    slot.strategy.on_rejected(s.instrument, now);
                }
                if s.intent.is_exit()
                    && let Some(slot) = self.slot_mut(s.strategy)
                    && reason != es_risk::RejectReason::ExitAlreadyPending
                {
                    slot.strategy.on_rejected(s.instrument, now);
                }
                return Err(reason.message().to_string());
            }
        };
        if s.intent.is_exit() {
            self.exits.entry(s.instrument).or_insert(ExitWant {
                strategy: s.strategy,
                reason: s.reason,
                since: self.now,
            });
        }
        self.send(req).await
    }

    pub(crate) async fn send(&mut self, req: OrderRequest) -> Result<es_core::OrderId, String> {
        let token = req.token();
        let (exchange, lot) = match self.market.as_ref().and_then(|m| m.instrument(token)) {
            Some(i) => (i.exchange.clone(), i.lot_size),
            None => return Err(format!("Unknown instrument {token}.")),
        };
        let id = self
            .oms
            .create(req.clone(), &exchange, lot, self.now)
            .map_err(|e| e.to_string())?;
        if let Err(e) = self.oms.mark_sent(id, self.now) {
            return Err(e.to_string());
        }
        let tag = self
            .oms
            .order(id)
            .map(|o| o.tag.clone())
            .unwrap_or_default();
        let result = self.broker.place(&req, &tag).await;
        let ok = result.is_ok();
        let ups = self.oms.on_place_result(id, result, self.now);
        self.handle_updates(ups).await;
        self.on_order_update(id);
        self.pump().await;
        if ok {
            Ok(id)
        } else {
            let msg = self
                .oms
                .order(id)
                .and_then(|o| o.message.clone())
                .unwrap_or_else(|| "Broker rejected the order.".into());
            Err(msg)
        }
    }

    // ------------------------------------------------------------ timer

    async fn on_timer(&mut self) {
        let now = self.now;
        if let Some(m) = self.market.as_mut() {
            m.advance(now);
        }
        self.broker.advance(now);
        self.pump().await;

        let mut sigs = Vec::new();
        if let Some(market) = self.market.as_ref() {
            for slot in &mut self.strategies {
                slot.strategy.on_timer(market, now, &mut sigs);
            }
        }
        for s in sigs {
            if let Err(e) = self.handle_signal(s).await {
                debug!(%e, "signal not sent");
            }
        }

        let book = self.book();
        let stale = self.feed_stale();
        let action = self.risk.monitor(&book.view(self.funds, stale), now);
        match action {
            Some(RiskAction::KillSwitch(cause)) => self.on_kill(cause).await,
            Some(RiskAction::Flatten) => {
                let reason = if self.risk.kill_state().is_triggered() {
                    ReasonCode::KillSwitch
                } else {
                    ReasonCode::Flatten
                };
                self.want_exit_all(reason, "risk");
            }
            None => {}
        }
        self.manage_orders().await;
    }

    /// Marks every open position for exit.
    pub(crate) fn want_exit_all(&mut self, reason: ReasonCode, strategy: &'static str) {
        let open: Vec<u32> = self
            .oms
            .positions()
            .filter(|p| p.qty != 0)
            .map(|p| p.token)
            .collect();
        for token in open {
            self.exits.entry(token).or_insert(ExitWant {
                strategy,
                reason,
                since: self.now,
            });
        }
    }

    pub(crate) async fn cancel_order(&mut self, id: es_core::OrderId) -> Result<(), String> {
        let bid = self
            .oms
            .begin_cancel(id, self.now)
            .map_err(|e| e.to_string())?;
        let r = self.broker.cancel(&bid).await;
        let err = r.as_ref().err().map(|e| e.to_string());
        self.oms.on_cancel_result(id, r, self.now);
        self.on_order_update(id);
        self.pump().await;
        match err {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }

    pub(crate) async fn cancel_working_entries(&mut self) {
        let ids: Vec<_> = self
            .oms
            .working()
            .into_iter()
            .filter(|id| self.oms.order(*id).is_some_and(|o| !o.is_exit))
            .collect();
        for id in ids {
            let _ = self.cancel_order(id).await;
        }
    }

    /// Kill switch: disable strategies, cancel entries, flatten.
    pub(crate) async fn on_kill(&mut self, cause: KillCause) {
        for s in &mut self.strategies {
            s.enabled = false;
        }
        self.cancel_working_entries().await;
        self.want_exit_all(ReasonCode::KillSwitch, "risk");
        self.alert(
            AlertLevel::Error,
            "kill_switch",
            match cause {
                KillCause::Manual => "Kill switch pressed. Strategies off, flattening.",
                KillCause::DailyMaxLoss => {
                    "Daily max loss hit. Kill switch on for the day, flattening."
                }
            },
        );
        self.manage_exits().await;
    }

    async fn manage_orders(&mut self) {
        let now = self.now;
        let t = self.cfg.timing.clone();
        // Stale working orders: entries time out, exits get re-priced.
        let stale: Vec<_> = self
            .oms
            .working()
            .into_iter()
            .filter_map(|id| {
                let o = self.oms.order(id)?;
                if o.state == es_oms::OrderState::CancelPending {
                    return None;
                }
                let age = now.ms_since(o.sent_at.unwrap_or(o.created_at));
                let limit = if o.is_exit {
                    t.exit_chase_ms
                } else {
                    t.entry_timeout_ms
                };
                (age >= limit).then_some(id)
            })
            .collect();
        for id in stale {
            let _ = self.cancel_order(id).await;
        }

        let timed_out = self.oms.check_timeouts(now, t.sent_timeout_ms);
        if !timed_out.is_empty() || self.oms.has_unknown() {
            match self.broker.orders().await {
                Ok(book) => {
                    let ups = self.oms.reconcile(&book, now);
                    self.handle_updates(ups).await;
                }
                Err(e) => warn!(%e, "reconcile: broker order book unavailable"),
            }
        }
        self.manage_exits().await;
    }

    /// Sends exit orders for positions marked for exit with nothing working.
    pub(crate) async fn manage_exits(&mut self) {
        let now = self.now;
        let wanted: Vec<(u32, ExitWant)> = self.exits.iter().map(|(k, v)| (*k, *v)).collect();
        for (token, want) in wanted {
            let open = self
                .oms
                .position(token)
                .map(|p| p.qty != 0)
                .unwrap_or(false);
            if !open {
                self.exits.remove(&token);
                continue;
            }
            let working = self.oms.working().into_iter().any(|id| {
                self.oms
                    .order(id)
                    .is_some_and(|o| o.token == token && o.is_exit)
            });
            if working {
                continue;
            }
            let Some(inst) = self.instrument_view(token) else {
                continue;
            };
            let book = self.book();
            let stale = self.feed_stale();
            let view = book.view(self.funds, stale);
            let decision = if now.ms_since(want.since) >= self.cfg.timing.market_exit_after_ms {
                self.risk
                    .check_market_exit(want.strategy, &inst, &view, now)
            } else {
                let s = Signal {
                    strategy: want.strategy,
                    instrument: token,
                    side: Side::Sell,
                    intent: Intent::Exit,
                    qty_lots: 0,
                    limit: None,
                    confidence: 1.0,
                    reason: want.reason,
                };
                self.risk.check(&s, &inst, &view, now)
            };
            self.journal
                .write("decision", now.0, &decision_json(&decision));
            if let Some(req) = decision.order()
                && let Err(e) = self.send(req).await
            {
                warn!(token, %e, "exit order not sent, will retry");
            }
        }
    }

    pub fn mark_replay_done(&mut self) {
        self.replay_done = true;
        self.feed.on_disconnect();
        self.journal.flush();
        self.alert(
            AlertLevel::Info,
            "source_done",
            "The tick source finished. Prices are frozen.",
        );
    }

    pub fn shutdown(&mut self) {
        self.journal.flush();
    }
}

pub(crate) fn side_dto(s: Side) -> SideDto {
    match s {
        Side::Buy => SideDto::Buy,
        Side::Sell => SideDto::Sell,
    }
}

fn decision_json(d: &RiskDecision) -> serde_json::Value {
    match d {
        RiskDecision::Approve(r) => serde_json::json!({"approve": r}),
        RiskDecision::Resize(r, why) => serde_json::json!({"resize": r, "why": why}),
        RiskDecision::Reject(why) => serde_json::json!({"reject": why}),
    }
}
