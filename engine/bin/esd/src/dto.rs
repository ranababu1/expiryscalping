//! Builds the client API snapshots from engine state. Read only.

use crate::engine::{Engine, side_dto};
use es_api::*;
use es_core::{InstrumentKind, OrderType, Ts};
use es_risk::{HhMm, KillCause, KillState, RiskLimits};

/// Strikes shown each side of ATM in the chain.
const CHAIN_STRIKES_EACH_SIDE: usize = 10;

pub fn limits_to_dto(l: &RiskLimits) -> RiskLimitsDto {
    RiskLimitsDto {
        daily_max_loss: l.daily_max_loss.0,
        per_trade_max_premium: l.per_trade_max_premium.0,
        per_trade_max_lots: l.per_trade_max_lots,
        max_open_positions: l.max_open_positions,
        max_trades_per_day: l.max_trades_per_day,
        max_orders_per_sec: l.max_orders_per_sec,
        max_orders_per_min: l.max_orders_per_min,
        entry_start: l.entry_start.to_string(),
        entry_end: l.entry_end.to_string(),
        flatten_at: l.flatten_at.to_string(),
        expiry_day_only: l.expiry_day_only,
        stale_after_ms: l.stale_after_ms,
        max_spread_bps: l.max_spread_bps,
        loss_streak_pause: l.loss_streak_pause,
        loss_pause_minutes: l.loss_pause_minutes,
        margin_check: l.margin_check,
        limit_buffer_ticks: l.limit_buffer_ticks,
        market_protection_pct: l.market_protection_pct,
    }
}

pub fn limits_from_dto(d: &RiskLimitsDto) -> Result<RiskLimits, String> {
    let t = |name: &str, s: &str| {
        HhMm::parse(s).ok_or_else(|| format!("{name}: expected HH:MM, got {s:?}"))
    };
    Ok(RiskLimits {
        daily_max_loss: es_core::Paise(d.daily_max_loss),
        per_trade_max_premium: es_core::Paise(d.per_trade_max_premium),
        per_trade_max_lots: d.per_trade_max_lots,
        max_open_positions: d.max_open_positions,
        max_trades_per_day: d.max_trades_per_day,
        max_orders_per_sec: d.max_orders_per_sec,
        max_orders_per_min: d.max_orders_per_min,
        entry_start: t("entry_start", &d.entry_start)?,
        entry_end: t("entry_end", &d.entry_end)?,
        flatten_at: t("flatten_at", &d.flatten_at)?,
        expiry_day_only: d.expiry_day_only,
        stale_after_ms: d.stale_after_ms,
        max_spread_bps: d.max_spread_bps,
        loss_streak_pause: d.loss_streak_pause,
        loss_pause_minutes: d.loss_pause_minutes,
        margin_check: d.margin_check,
        limit_buffer_ticks: d.limit_buffer_ticks,
        market_protection_pct: d.market_protection_pct,
    })
}

fn kill_dto(k: KillState) -> KillDto {
    match k {
        KillState::Armed => KillDto {
            triggered: false,
            cause: None,
            at: None,
        },
        KillState::Triggered { cause, at, .. } => KillDto {
            triggered: true,
            cause: Some(
                match cause {
                    KillCause::Manual => "manual",
                    KillCause::DailyMaxLoss => "daily_max_loss",
                }
                .into(),
            ),
            at: Some(at.millis()),
        },
    }
}

fn snake(s: impl std::fmt::Debug) -> String {
    let raw = format!("{s:?}");
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Engine {
    pub(crate) fn market_open(&self) -> bool {
        let m = self.now.ist_minute_of_day();
        let h = self.cfg.market.hours;
        self.now.0 > 0 && m >= h.open.0 && m < h.close.0
    }

    pub fn status_dto(&self) -> StatusDto {
        let (p50, p99, n) = self.latency.summary();
        StatusDto {
            mode: Mode::Paper,
            source: self.opts.source.clone(),
            engine_version: self.opts.version.clone(),
            engine_time: self.now.millis(),
            market_open: self.market_open(),
            feed: FeedDto {
                connected: self.feed.connected(),
                stale: self.feed_stale(),
                last_tick_age_ms: self.feed.last_frame().map(|t| self.now.ms_since(t)),
                frames: self.feed.frames(),
            },
            kite: "not connected (Phase 2)".into(),
            ai_paused: self.ai_paused,
            kill: kill_dto(self.risk.kill_state()),
            latency: LatencyDto {
                p50_us: p50,
                p99_us: p99,
                samples: n,
            },
            replay_done: self.replay_done,
        }
    }

    fn quote_dto(&self, token: u32) -> Option<OptionQuoteDto> {
        let m = self.market.as_ref()?;
        let i = m.instrument(token)?;
        let q = m.quote(token).copied().unwrap_or_default();
        Some(OptionQuoteDto {
            token,
            symbol: i.tradingsymbol.clone(),
            ltp: q.ltp.0,
            bid: q.best_bid().map(|p| p.0),
            ask: q.best_ask().map(|p| p.0),
            oi: q.oi,
            oi_change: q.oi_change(),
            iv_pct: m.iv(token).map(|v| (v * 10_000.0).round() / 100.0),
        })
    }

    pub fn chain_dto(&self) -> ChainDto {
        let underlyings: Vec<String> = self
            .cfg
            .market
            .underlyings
            .iter()
            .map(|u| u.name.clone())
            .collect();
        let u = self.chain_underlying.as_str();
        let mut dto = ChainDto {
            underlying: u.to_string(),
            underlyings,
            expiry: None,
            spot: None,
            atm_strike: None,
            lot_size: 0,
            rows: Vec::new(),
        };
        let Some(m) = self.market.as_ref() else {
            return dto;
        };
        dto.spot = m.spot(u).map(|p| p.0);
        let Some(expiry) = m.nearest_expiry(u) else {
            return dto;
        };
        dto.expiry = Some(expiry.to_string());
        let atm = m.atm(u, expiry);
        dto.atm_strike = atm.as_ref().map(|r| r.strike.0);
        let rows = m.chain(u, expiry);
        let centre = atm
            .and_then(|a| rows.iter().position(|r| r.strike == a.strike))
            .unwrap_or(rows.len() / 2);
        let lo = centre.saturating_sub(CHAIN_STRIKES_EACH_SIDE);
        let hi = (centre + CHAIN_STRIKES_EACH_SIDE + 1).min(rows.len());
        for r in rows.get(lo..hi).unwrap_or(&[]) {
            let ce = r.ce.and_then(|t| self.quote_dto(t));
            let pe = r.pe.and_then(|t| self.quote_dto(t));
            if dto.lot_size == 0 {
                dto.lot_size =
                    r.ce.or(r.pe)
                        .and_then(|t| m.instrument(t))
                        .map(|i| i.lot_size)
                        .unwrap_or(0);
            }
            dto.rows.push(ChainRowDto {
                strike: r.strike.0,
                ce,
                pe,
            });
        }
        dto
    }

    pub fn positions_dto(&self) -> PositionsDto {
        let mut positions = Vec::new();
        let (mut realised, mut unrealised, mut charges) = (0i64, 0i64, 0i64);
        for p in self.oms.positions() {
            let ltp = self.mark(p.token).unwrap_or(p.last_price);
            let lot = self
                .market
                .as_ref()
                .and_then(|m| m.instrument(p.token))
                .map(|i| i.lot_size.max(1))
                .unwrap_or(1);
            let u = p.unrealised(ltp);
            realised += p.realised().0;
            unrealised += u.0;
            charges += p.charges.0;
            positions.push(PositionDto {
                token: p.token,
                symbol: self.symbol(p.token),
                qty: p.qty,
                lots: (p.qty.unsigned_abs() / lot as u64) as u32,
                avg_price: p.avg_price.0,
                ltp: ltp.0,
                unrealised: u.0,
                realised: p.realised().0,
                charges: p.charges.0,
            });
        }
        PositionsDto {
            positions,
            pnl: PnlDto {
                realised,
                unrealised,
                total: realised + unrealised,
                charges,
                available_funds: self.funds.0,
            },
        }
    }

    pub fn orders_dto(&self) -> OrdersDto {
        let mut orders: Vec<OrderDto> = self
            .oms
            .orders()
            .map(|o| OrderDto {
                id: o.id.0,
                tag: o.tag.clone(),
                broker_id: o.broker_id.as_ref().map(|b| b.0.clone()),
                token: o.token,
                symbol: self.symbol(o.token),
                side: side_dto(o.side),
                qty: o.qty,
                filled: o.filled,
                limit: o.limit.map(|p| p.0),
                avg_fill_price: o.avg_fill_price.0,
                order_type: match o.order_type {
                    OrderType::Limit => "limit",
                    OrderType::Market => "market",
                }
                .into(),
                is_exit: o.is_exit,
                strategy: o.strategy.to_string(),
                reason: snake(o.reason),
                state: snake(o.state),
                working: o.state.is_working(),
                modifications: o.modifications,
                message: o.message.clone(),
                created_at: o.created_at.millis(),
                updated_at: o.updated_at.millis(),
            })
            .collect();
        orders.sort_by_key(|o| std::cmp::Reverse(o.id));
        OrdersDto { orders }
    }

    pub fn risk_dto(&mut self) -> RiskDto {
        let book = self.book();
        let stale = self.feed_stale();
        let now = self.now;
        let u = self.risk.usage(&book.view(self.funds, stale), now);
        RiskDto {
            limits: limits_to_dto(self.risk.limits()),
            usage: RiskUsageDto {
                pnl: u.pnl.0,
                loss_used: u.loss_used.0,
                trades_today: u.trades_today,
                open_positions: u.open_positions,
                orders_last_sec: u.orders_last_sec,
                orders_last_min: u.orders_last_min,
                loss_streak: u.loss_streak,
                paused_until: u.paused_until.map(Ts::millis),
            },
            kill: kill_dto(u.kill),
            rejections: u
                .last_rejections
                .iter()
                .rev()
                .map(|(ts, token, strategy, reason)| RejectionDto {
                    ts: ts.millis(),
                    token: *token,
                    strategy: strategy.to_string(),
                    reason: reason.message().to_string(),
                })
                .collect(),
            market_open: self.market_open(),
        }
    }

    pub fn strategies_dto(&self) -> StrategiesDto {
        StrategiesDto {
            strategies: self
                .strategies
                .iter()
                .map(|s| StrategyDto {
                    id: s.strategy.id().to_string(),
                    enabled: s.enabled,
                    status: s.strategy.status(),
                    description: s.description.to_string(),
                })
                .collect(),
            ai_paused: self.ai_paused,
        }
    }

    /// Publishes every snapshot stream.
    pub fn publish_all(&mut self) {
        if self.publisher.is_none() {
            return;
        }
        let msgs = [
            ServerMsg::Status(self.status_dto()),
            ServerMsg::Chain(self.chain_dto()),
            ServerMsg::Positions(self.positions_dto()),
            ServerMsg::Orders(self.orders_dto()),
            ServerMsg::Risk(self.risk_dto()),
            ServerMsg::Strategies(self.strategies_dto()),
        ];
        if let Some(p) = &self.publisher {
            for m in msgs {
                p.publish(m);
            }
        }
    }

    /// True if this token is an option the manual ticket may trade.
    pub(crate) fn is_option(&self, token: u32) -> bool {
        self.market
            .as_ref()
            .and_then(|m| m.instrument(token))
            .is_some_and(|i| matches!(i.kind, InstrumentKind::Ce | InstrumentKind::Pe))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_round_trip_through_dto() {
        let l = RiskLimits::default();
        assert_eq!(limits_from_dto(&limits_to_dto(&l)).unwrap(), l);
    }

    #[test]
    fn bad_time_in_dto_is_an_error() {
        let mut d = limits_to_dto(&RiskLimits::default());
        d.flatten_at = "25:00".into();
        assert!(limits_from_dto(&d).is_err());
    }

    #[test]
    fn snake_case_names() {
        assert_eq!(
            snake(es_oms::OrderState::PartiallyFilled),
            "partially_filled"
        );
        assert_eq!(snake(es_core::ReasonCode::StopLoss), "stop_loss");
    }
}
