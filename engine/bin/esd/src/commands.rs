//! Client commands (desktop app). Each one goes through the same paths as
//! strategy signals: manual orders are signals checked by the risk engine.
//! Every state-changing command is written to the audit log.

use crate::config::RISK_OVERRIDE_FILE;
use crate::dto::limits_from_dto;
use crate::engine::{Engine, ExitWant, MANUAL};
use es_api::{AlertLevel, ApiCommand, ClientMsg, ErrorCode, Mode, ServerMsg};
use es_core::{Intent, OrderId, Paise, ReasonCode, Side, Signal};
use es_risk::KillCause;
use std::io::Write;
use tracing::{info, warn};

type Reply = Result<Option<String>, (ErrorCode, String)>;

fn err<T>(code: ErrorCode, msg: impl Into<String>) -> Result<T, (ErrorCode, String)> {
    Err((code, msg.into()))
}

impl Engine {
    pub async fn on_command(&mut self, cmd: ApiCommand) {
        let ApiCommand { device, msg, reply } = cmd;
        let req_id = msg.req_id();
        let result = self.execute(&device, msg).await;
        let out = match (result, req_id) {
            (Ok(message), Some(req_id)) => ServerMsg::Ack { req_id, message },
            (Ok(_), None) => return,
            (Err((code, message)), req_id) => ServerMsg::Error {
                req_id,
                code,
                message,
            },
        };
        let _ = reply.send(out);
        // Make the effect visible right away.
        self.publish_all();
    }

    fn audit(&mut self, device: &str, action: &str, detail: serde_json::Value) {
        info!(device, action, %detail, "audit");
        self.journal
            .write("audit", self.now.0, &(device, action, &detail));
        let Some(dir) = self.opts.data_dir.as_ref() else {
            return;
        };
        let line = serde_json::json!({
            "engine_time": self.now.millis(),
            "device": device,
            "action": action,
            "detail": detail,
        });
        let path = dir.join("audit.jsonl");
        let res = std::fs::create_dir_all(dir).and_then(|_| {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(f, "{line}")
        });
        if let Err(e) = res {
            warn!(%e, "audit log write failed");
        }
    }

    pub async fn execute(&mut self, device: &str, msg: ClientMsg) -> Reply {
        use ClientMsg::*;
        match msg {
            Auth { .. } | Subscribe { .. } | Ping { .. } => Ok(None),
            SetStrategy { id, enabled, .. } => {
                if enabled && self.risk.kill_state().is_triggered() {
                    return err(
                        ErrorCode::NotAllowed,
                        "Kill switch is on. Re-arm it before enabling strategies.",
                    );
                }
                let Some(slot) = self.slot_mut(&id) else {
                    return err(ErrorCode::NotFound, format!("No strategy {id:?}."));
                };
                slot.enabled = enabled;
                self.audit(
                    device,
                    "set_strategy",
                    serde_json::json!({"id": id, "enabled": enabled}),
                );
                Ok(None)
            }
            PauseAi { paused, .. } => {
                self.ai_paused = paused;
                self.audit(device, "pause_ai", serde_json::json!({"paused": paused}));
                Ok(Some(
                    if paused {
                        "AI and strategy entries paused. Exits still run."
                    } else {
                        "Entries resumed."
                    }
                    .into(),
                ))
            }
            SetMode { mode, .. } => match mode {
                Mode::Paper => Ok(Some("Paper mode.".into())),
                Mode::Live => err(
                    ErrorCode::NotAvailable,
                    "Live trading arrives in Phase 3. This engine runs in paper mode only.",
                ),
            },
            SetRisk {
                limits, confirm, ..
            } => {
                let new = limits_from_dto(&limits).map_err(|e| (ErrorCode::BadRequest, e))?;
                let old = *self.risk.limits();
                let book = self.book();
                let stale = self.feed_stale();
                let now = self.now;
                self.risk
                    .set_limits(new, &book.view(self.funds, stale), confirm, now)
                    .map_err(|e| (ErrorCode::NotAllowed, e.to_string()))?;
                self.audit(
                    device,
                    "set_risk",
                    serde_json::json!({"old": old, "new": new, "confirm": confirm}),
                );
                self.save_limits();
                Ok(Some("Risk limits saved.".into()))
            }
            Kill { .. } => {
                self.audit(device, "kill", serde_json::json!({}));
                if self.risk.trigger_kill(KillCause::Manual, self.now) {
                    self.on_kill(KillCause::Manual).await;
                } else {
                    // Already on: flatten again in case something is left.
                    self.want_exit_all(ReasonCode::KillSwitch, "risk");
                    self.manage_exits().await;
                }
                Ok(Some("Kill switch on. Flattening.".into()))
            }
            Rearm { .. } => {
                let book = self.book();
                let stale = self.feed_stale();
                let now = self.now;
                self.risk
                    .rearm(&book.view(self.funds, stale), now)
                    .map_err(|e| (ErrorCode::NotAllowed, e.to_string()))?;
                self.audit(device, "rearm", serde_json::json!({}));
                self.alert(
                    AlertLevel::Info,
                    "rearmed",
                    "Kill switch re-armed. Strategies stay off until you enable them.",
                );
                Ok(Some("Re-armed.".into()))
            }
            Place {
                token, lots, limit, ..
            } => {
                if !self.is_option(token) {
                    return err(ErrorCode::BadRequest, "Only options can be traded.");
                }
                self.audit(
                    device,
                    "place",
                    serde_json::json!({"token": token, "lots": lots, "limit": limit}),
                );
                let s = Signal {
                    strategy: MANUAL,
                    instrument: token,
                    side: Side::Buy,
                    intent: Intent::Enter,
                    qty_lots: lots,
                    limit: limit.map(Paise),
                    confidence: 1.0,
                    reason: ReasonCode::Manual,
                };
                match self.handle_signal(s).await {
                    Ok(id) => Ok(Some(format!("Order {} sent.", id.0))),
                    Err(e) => err(ErrorCode::RiskRejected, e),
                }
            }
            Modify {
                order_id,
                lots,
                limit,
                ..
            } => {
                let id = OrderId(order_id);
                let Some(view) = self.oms.open_order_view(id) else {
                    return err(ErrorCode::NotFound, "No working order with that id.");
                };
                let now = self.now;
                let approved = self
                    .risk
                    .check_modify(&view, lots, Paise(limit), now)
                    .map_err(|r| (ErrorCode::RiskRejected, r.message().to_string()))?;
                let bid = self
                    .oms
                    .begin_modify(id, now)
                    .map_err(|e| (ErrorCode::NotAllowed, e.to_string()))?;
                self.audit(
                    device,
                    "modify",
                    serde_json::json!({"order_id": order_id, "lots": lots, "limit": limit}),
                );
                let r = self.broker_modify(&bid, approved).await;
                let fail = r.as_ref().err().map(|e| e.to_string());
                self.oms.on_modify_result(id, r, self.now);
                self.pump().await;
                match fail {
                    None => Ok(Some("Modified.".into())),
                    Some(e) => err(ErrorCode::NotAllowed, e),
                }
            }
            Cancel { order_id, .. } => {
                let id = OrderId(order_id);
                let token = self.oms.order(id).map(|o| (o.token, o.is_exit));
                self.audit(device, "cancel", serde_json::json!({"order_id": order_id}));
                // Cancelling a manual exit means "stop trying"; risk exits
                // (kill switch, flatten time) keep going.
                if let Some((t, true)) = token
                    && self.exits.get(&t).is_some_and(|w| w.strategy == MANUAL)
                {
                    self.exits.remove(&t);
                }
                self.cancel_order(id)
                    .await
                    .map_err(|e| (ErrorCode::NotAllowed, e))?;
                Ok(Some("Cancel sent.".into()))
            }
            Exit { token, .. } => {
                if self.oms.position(token).is_none_or(|p| p.qty == 0) {
                    return err(ErrorCode::NotFound, "No open position in that instrument.");
                }
                self.audit(device, "exit", serde_json::json!({"token": token}));
                self.exits.entry(token).or_insert(ExitWant {
                    strategy: MANUAL,
                    reason: ReasonCode::Manual,
                    since: self.now,
                });
                self.manage_exits().await;
                Ok(Some("Exit sent.".into()))
            }
            ExitAll { .. } => {
                self.audit(device, "exit_all", serde_json::json!({}));
                self.cancel_working_entries().await;
                self.want_exit_all(ReasonCode::Manual, MANUAL);
                self.manage_exits().await;
                Ok(Some("Closing everything.".into()))
            }
            SelectChain { underlying, .. } => {
                if !self
                    .cfg
                    .market
                    .underlyings
                    .iter()
                    .any(|u| u.name == underlying)
                {
                    return err(
                        ErrorCode::NotFound,
                        format!("Unknown underlying {underlying}."),
                    );
                }
                self.chain_underlying = underlying;
                Ok(None)
            }
            KiteLoginUrl { .. } | KiteRequestToken { .. } => err(
                ErrorCode::NotAvailable,
                "Kite login arrives in Phase 2. Phase 1 runs on simulated or replayed ticks.",
            ),
        }
    }

    async fn broker_modify(
        &self,
        bid: &es_broker::BrokerOrderId,
        m: es_risk::ApprovedModify,
    ) -> Result<(), es_broker::BrokerError> {
        use es_broker::Broker;
        self.broker.modify(bid, m).await
    }

    fn save_limits(&mut self) {
        let Some(dir) = self.opts.data_dir.clone() else {
            return;
        };
        let res = serde_json::to_string_pretty(self.risk.limits())
            .map_err(std::io::Error::other)
            .and_then(|text| {
                std::fs::create_dir_all(&dir)?;
                let tmp = dir.join(format!("{RISK_OVERRIDE_FILE}.tmp"));
                std::fs::write(&tmp, text)?;
                std::fs::rename(&tmp, dir.join(RISK_OVERRIDE_FILE))
            });
        if let Err(e) = res {
            self.alert(
                AlertLevel::Error,
                "risk_save",
                format!("Risk limits applied but not saved to disk: {e}"),
            );
        }
    }
}
