//! Paper broker: same trait, same order manager, same risk engine as live.
//!
//! - An order becomes eligible to fill after a simulated latency.
//! - It fills against the opposite side of the latest 5-level depth,
//!   walking levels for size, plus a configurable slippage.
//! - Thin depth gives partial fills; the rest keeps working on later ticks.
//! - Each depth snapshot is used once per order, so the same displayed
//!   quantity is not filled twice.
//! - Market orders are bounded by market protection.

use crate::*;
use es_core::{Depth5, OrderType, Paise, Side, Tick, Ts};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperConfig {
    /// Simulated time from placing an order to it reaching the exchange.
    pub latency_ms: i64,
    /// Extra price paid (buys) or given up (sells) per fill, in paise.
    pub slippage_paise: i64,
    /// Starting funds for the paper account.
    pub capital: Paise,
}

impl Default for PaperConfig {
    fn default() -> Self {
        PaperConfig {
            latency_ms: 10,
            slippage_paise: 0,
            capital: Paise::from_rupees(100_000),
        }
    }
}

#[derive(Debug, Clone)]
struct PaperOrder {
    id: BrokerOrderId,
    tag: String,
    trades: u32,
    token: u32,
    side: Side,
    qty: u32,
    filled: u32,
    limit: Option<Paise>,
    order_type: OrderType,
    protection_pct: u32,
    /// Exchange fills come in whole lots.
    lot: u32,
    eligible_at: Ts,
    /// Receive time of the depth snapshot last matched, to use each once.
    last_matched: Option<Ts>,
    status: BrokerOrderStatus,
}

#[derive(Default)]
struct Inner {
    now: Ts,
    seq: u64,
    orders: Vec<PaperOrder>,
    depth: HashMap<u32, (Depth5, Ts)>,
    positions: HashMap<u32, i64>,
    cash: Paise,
    events: Vec<BrokerEvent>,
}

pub struct PaperBroker {
    cfg: PaperConfig,
    inner: Mutex<Inner>,
}

impl PaperBroker {
    pub fn new(cfg: PaperConfig) -> PaperBroker {
        PaperBroker {
            cfg,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock only means another thread panicked mid-update; the
        // data is still the best state we have.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Feeds a tick: updates depth and engine time, then matches orders.
    pub fn on_tick(&self, t: &Tick) {
        let mut g = self.lock();
        g.now = g.now.max(t.recv);
        if let Some(d) = t.depth {
            g.depth.insert(t.token, (d, t.recv));
        }
        self.match_orders(&mut g);
    }

    /// Advances time without a tick (orders whose latency elapsed can fill).
    pub fn advance(&self, now: Ts) {
        let mut g = self.lock();
        g.now = g.now.max(now);
        self.match_orders(&mut g);
    }

    /// Moves pending broker events into `out`.
    pub fn drain_events(&self, out: &mut Vec<BrokerEvent>) {
        out.append(&mut self.lock().events);
    }

    fn match_orders(&self, g: &mut Inner) {
        let now = g.now;
        let slip = self.cfg.slippage_paise;
        let mut fills: Vec<(usize, u32, Paise)> = Vec::new();
        for (i, o) in g.orders.iter_mut().enumerate() {
            if o.status != BrokerOrderStatus::Open || now < o.eligible_at {
                continue;
            }
            let Some(&(depth, depth_ts)) = g.depth.get(&o.token) else {
                continue;
            };
            if o.last_matched == Some(depth_ts) {
                continue;
            }
            o.last_matched = Some(depth_ts);
            let levels = match o.side {
                Side::Buy => depth.asks,
                Side::Sell => depth.bids,
            };
            let bound = match (o.order_type, o.limit) {
                (OrderType::Limit, Some(l)) => l,
                _ => {
                    // Market protection: at most protection% away from best.
                    let best = levels[0].price;
                    let pct = o.protection_pct as i64;
                    match o.side {
                        Side::Buy => Paise(best.0 + best.0 * pct / 100),
                        Side::Sell => Paise(best.0 - best.0 * pct / 100),
                    }
                }
            };
            let ok = |p: Paise| match o.side {
                Side::Buy => p <= bound,
                Side::Sell => p >= bound,
            };
            let usable: Vec<(u32, Paise)> = levels
                .iter()
                .filter(|l| l.qty > 0 && l.price.0 > 0)
                .take_while(|l| ok(l.price))
                .map(|l| (l.qty, l.price))
                .collect();
            let available: u32 = usable.iter().map(|(q, _)| *q).sum();
            let want = (o.qty - o.filled).min(available);
            let mut remaining = want - want % o.lot;
            for (lq, lp) in usable {
                if remaining == 0 {
                    break;
                }
                let q = remaining.min(lq);
                let price = match o.side {
                    Side::Buy => Paise(lp.0 + slip),
                    Side::Sell => Paise((lp.0 - slip).max(1)),
                };
                fills.push((i, q, price));
                remaining -= q;
            }
        }
        for (i, q, price) in fills {
            let (id, trade_id, token, side, done) = {
                let o = &mut g.orders[i];
                o.filled += q;
                o.trades += 1;
                let trade_id = format!("{}-T{}", o.id.0, o.trades);
                if o.filled == o.qty {
                    o.status = BrokerOrderStatus::Complete;
                }
                (o.id.clone(), trade_id, o.token, o.side, o.filled == o.qty)
            };
            *g.positions.entry(token).or_insert(0) += side.sign() * q as i64;
            g.cash -= price.times(side.sign() * q as i64);
            g.events.push(BrokerEvent::Fill {
                id: id.clone(),
                trade_id,
                qty: q,
                price,
                ts: now,
            });
            if done {
                g.events.push(BrokerEvent::Complete { id });
            }
        }
    }
}

#[async_trait]
impl Broker for PaperBroker {
    async fn place(&self, req: &OrderRequest, tag: &str) -> Result<BrokerOrderId, BrokerError> {
        let mut g = self.lock();
        if req.qty() == 0 {
            return Err(BrokerError::Rejected("zero quantity".into()));
        }
        g.seq += 1;
        let id = BrokerOrderId(format!("PAPER-{}", g.seq));
        let eligible_at = g.now.plus_ms(self.cfg.latency_ms);
        g.orders.push(PaperOrder {
            id: id.clone(),
            tag: tag.to_string(),
            trades: 0,
            token: req.token(),
            side: req.side(),
            qty: req.qty(),
            filled: 0,
            limit: req.limit(),
            order_type: req.order_type(),
            protection_pct: req.market_protection_pct(),
            lot: (req.qty() / req.lots().max(1)).max(1),
            eligible_at,
            last_matched: None,
            status: BrokerOrderStatus::Open,
        });
        if self.cfg.latency_ms <= 0 {
            self.match_orders(&mut g);
        }
        Ok(id)
    }

    async fn modify(&self, id: &BrokerOrderId, m: ApprovedModify) -> Result<(), BrokerError> {
        let mut g = self.lock();
        let o = g
            .orders
            .iter_mut()
            .find(|o| &o.id == id)
            .ok_or_else(|| BrokerError::UnknownOrder(id.0.clone()))?;
        if o.status != BrokerOrderStatus::Open {
            return Err(BrokerError::NotOpen);
        }
        if m.qty() < o.filled {
            return Err(BrokerError::Rejected(
                "new quantity is below filled quantity".into(),
            ));
        }
        o.qty = m.qty();
        o.limit = Some(m.limit());
        o.order_type = OrderType::Limit;
        o.last_matched = None;
        let done = o.filled == o.qty;
        if done {
            o.status = BrokerOrderStatus::Complete;
        }
        let ev = BrokerEvent::Modified {
            id: id.clone(),
            qty: m.qty(),
            limit: m.limit(),
        };
        g.events.push(ev);
        if done {
            g.events.push(BrokerEvent::Complete { id: id.clone() });
        }
        self.match_orders(&mut g);
        Ok(())
    }

    async fn cancel(&self, id: &BrokerOrderId) -> Result<(), BrokerError> {
        let mut g = self.lock();
        let o = g
            .orders
            .iter_mut()
            .find(|o| &o.id == id)
            .ok_or_else(|| BrokerError::UnknownOrder(id.0.clone()))?;
        if o.status != BrokerOrderStatus::Open {
            return Err(BrokerError::NotOpen);
        }
        o.status = BrokerOrderStatus::Cancelled;
        g.events.push(BrokerEvent::Cancelled { id: id.clone() });
        Ok(())
    }

    async fn orders(&self) -> Result<Vec<BrokerOrder>, BrokerError> {
        Ok(self
            .lock()
            .orders
            .iter()
            .map(|o| BrokerOrder {
                id: o.id.clone(),
                tag: o.tag.clone(),
                token: o.token,
                side: o.side,
                qty: o.qty,
                filled: o.filled,
                limit: o.limit,
                order_type: o.order_type,
                status: o.status,
            })
            .collect())
    }

    async fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        let g = self.lock();
        let mut v: Vec<Position> = g
            .positions
            .iter()
            .filter(|(_, q)| **q != 0)
            .map(|(t, q)| Position { token: *t, qty: *q })
            .collect();
        v.sort_by_key(|p| p.token);
        Ok(v)
    }

    async fn margins(&self) -> Result<Margins, BrokerError> {
        let g = self.lock();
        Ok(Margins {
            available: self.cfg.capital + g.cash,
        })
    }
}

#[cfg(test)]
mod tests;
