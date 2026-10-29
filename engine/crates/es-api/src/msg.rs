//! Every message between the engine and its clients. Defined once here and
//! exported to TypeScript (`app/src/lib/api/types`) so the app and the
//! engine cannot drift. Money is integer paise, times are milliseconds since
//! the Unix epoch, as plain numbers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mode {
    Paper,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SideDto {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Stream {
    Status,
    Chain,
    Positions,
    Orders,
    Risk,
    Strategies,
    Alerts,
}

// ---------------------------------------------------------------- client

/// Client to server. The first message on a connection must be `auth`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum ClientMsg {
    Auth {
        token: String,
    },
    Subscribe {
        req_id: u32,
        streams: Vec<Stream>,
    },
    SetStrategy {
        req_id: u32,
        id: String,
        enabled: bool,
    },
    /// Pause or resume automated entries. Exits keep working.
    PauseAi {
        req_id: u32,
        paused: bool,
    },
    SetMode {
        req_id: u32,
        mode: Mode,
    },
    SetRisk {
        req_id: u32,
        limits: RiskLimitsDto,
        /// Required to loosen limits during market hours (book must be flat).
        confirm: bool,
    },
    Kill {
        req_id: u32,
    },
    Rearm {
        req_id: u32,
    },
    /// Manual entry. Goes through the risk engine like any signal.
    Place {
        req_id: u32,
        token: u32,
        lots: u32,
        /// Limit price in paise. `null` for a marketable limit.
        #[ts(type = "number | null")]
        limit: Option<i64>,
    },
    Modify {
        req_id: u32,
        #[ts(type = "number")]
        order_id: u64,
        lots: u32,
        #[ts(type = "number")]
        limit: i64,
    },
    Cancel {
        req_id: u32,
        #[ts(type = "number")]
        order_id: u64,
    },
    /// Exit one position (always allowed).
    Exit {
        req_id: u32,
        token: u32,
    },
    /// Exit every position (always allowed). Does not trigger the kill switch.
    ExitAll {
        req_id: u32,
    },
    SelectChain {
        req_id: u32,
        underlying: String,
    },
    KiteLoginUrl {
        req_id: u32,
    },
    KiteRequestToken {
        req_id: u32,
        request_token: String,
    },
    Ping {
        req_id: u32,
    },
}

impl ClientMsg {
    pub fn req_id(&self) -> Option<u32> {
        use ClientMsg::*;
        match self {
            Auth { .. } => None,
            Subscribe { req_id, .. }
            | SetStrategy { req_id, .. }
            | PauseAi { req_id, .. }
            | SetMode { req_id, .. }
            | SetRisk { req_id, .. }
            | Kill { req_id }
            | Rearm { req_id }
            | Place { req_id, .. }
            | Modify { req_id, .. }
            | Cancel { req_id, .. }
            | Exit { req_id, .. }
            | ExitAll { req_id }
            | SelectChain { req_id, .. }
            | KiteLoginUrl { req_id }
            | KiteRequestToken { req_id, .. }
            | Ping { req_id } => Some(*req_id),
        }
    }
}

// ---------------------------------------------------------------- server

/// Server to client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMsg {
    Hello {
        protocol: u32,
        engine_version: String,
        device: String,
    },
    Ack {
        req_id: u32,
        message: Option<String>,
    },
    Error {
        req_id: Option<u32>,
        code: ErrorCode,
        message: String,
    },
    Status(StatusDto),
    Chain(ChainDto),
    Positions(PositionsDto),
    Orders(OrdersDto),
    Fill(FillDto),
    Risk(RiskDto),
    Strategies(StrategiesDto),
    Alert(AlertDto),
}

impl ServerMsg {
    pub fn stream(&self) -> Option<Stream> {
        match self {
            ServerMsg::Status(_) => Some(Stream::Status),
            ServerMsg::Chain(_) => Some(Stream::Chain),
            ServerMsg::Positions(_) => Some(Stream::Positions),
            ServerMsg::Orders(_) | ServerMsg::Fill(_) => Some(Stream::Orders),
            ServerMsg::Risk(_) => Some(Stream::Risk),
            ServerMsg::Strategies(_) => Some(Stream::Strategies),
            ServerMsg::Alert(_) => Some(Stream::Alerts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    Unauthorized,
    BadRequest,
    RiskRejected,
    RateLimited,
    NotFound,
    NotAllowed,
    NotAvailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeedDto {
    pub connected: bool,
    pub stale: bool,
    #[ts(type = "number | null")]
    pub last_tick_age_ms: Option<i64>,
    #[ts(type = "number")]
    pub frames: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LatencyDto {
    /// Tick processing time (market state, strategies, risk), microseconds.
    pub p50_us: u32,
    pub p99_us: u32,
    pub samples: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusDto {
    pub mode: Mode,
    /// `sim`, `replay` or `kite`.
    pub source: String,
    pub engine_version: String,
    /// Engine (market) time.
    #[ts(type = "number")]
    pub engine_time: i64,
    pub market_open: bool,
    pub feed: FeedDto,
    /// `not_connected` until the Kite broker exists (Phase 2 and 3).
    pub kite: String,
    pub ai_paused: bool,
    pub kill: KillDto,
    pub latency: LatencyDto,
    pub replay_done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KillDto {
    pub triggered: bool,
    /// `manual` or `daily_max_loss`.
    pub cause: Option<String>,
    #[ts(type = "number | null")]
    pub at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OptionQuoteDto {
    pub token: u32,
    pub symbol: String,
    #[ts(type = "number")]
    pub ltp: i64,
    #[ts(type = "number | null")]
    pub bid: Option<i64>,
    #[ts(type = "number | null")]
    pub ask: Option<i64>,
    #[ts(type = "number")]
    pub oi: u64,
    #[ts(type = "number")]
    pub oi_change: i64,
    /// Annualised implied volatility in percent.
    pub iv_pct: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChainRowDto {
    #[ts(type = "number")]
    pub strike: i64,
    pub ce: Option<OptionQuoteDto>,
    pub pe: Option<OptionQuoteDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChainDto {
    pub underlying: String,
    pub underlyings: Vec<String>,
    /// `YYYY-MM-DD`.
    pub expiry: Option<String>,
    #[ts(type = "number | null")]
    pub spot: Option<i64>,
    #[ts(type = "number | null")]
    pub atm_strike: Option<i64>,
    pub lot_size: u32,
    pub rows: Vec<ChainRowDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PositionDto {
    pub token: u32,
    pub symbol: String,
    #[ts(type = "number")]
    pub qty: i64,
    pub lots: u32,
    #[ts(type = "number")]
    pub avg_price: i64,
    #[ts(type = "number")]
    pub ltp: i64,
    #[ts(type = "number")]
    pub unrealised: i64,
    /// Realised today, net of charges.
    #[ts(type = "number")]
    pub realised: i64,
    #[ts(type = "number")]
    pub charges: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PnlDto {
    #[ts(type = "number")]
    pub realised: i64,
    #[ts(type = "number")]
    pub unrealised: i64,
    #[ts(type = "number")]
    pub total: i64,
    #[ts(type = "number")]
    pub charges: i64,
    #[ts(type = "number")]
    pub available_funds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PositionsDto {
    pub positions: Vec<PositionDto>,
    pub pnl: PnlDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrderDto {
    #[ts(type = "number")]
    pub id: u64,
    pub tag: String,
    pub broker_id: Option<String>,
    pub token: u32,
    pub symbol: String,
    pub side: SideDto,
    pub qty: u32,
    pub filled: u32,
    #[ts(type = "number | null")]
    pub limit: Option<i64>,
    #[ts(type = "number")]
    pub avg_fill_price: i64,
    /// `limit` or `market`.
    pub order_type: String,
    pub is_exit: bool,
    pub strategy: String,
    pub reason: String,
    /// `created`, `sent`, `open`, `partially_filled`, `modify_pending`,
    /// `cancel_pending`, `complete`, `rejected`, `cancelled`, `unknown`.
    pub state: String,
    pub working: bool,
    pub modifications: u32,
    pub message: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OrdersDto {
    pub orders: Vec<OrderDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FillDto {
    #[ts(type = "number")]
    pub order_id: u64,
    pub token: u32,
    pub symbol: String,
    pub side: SideDto,
    pub qty: u32,
    #[ts(type = "number")]
    pub price: i64,
    #[ts(type = "number")]
    pub ts: i64,
    pub strategy: String,
}

/// Mirrors `es_risk::RiskLimits`. Times are `"HH:MM"` IST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RiskLimitsDto {
    #[ts(type = "number")]
    pub daily_max_loss: i64,
    #[ts(type = "number")]
    pub per_trade_max_premium: i64,
    pub per_trade_max_lots: u32,
    pub max_open_positions: u32,
    pub max_trades_per_day: u32,
    pub max_orders_per_sec: u32,
    pub max_orders_per_min: u32,
    pub entry_start: String,
    pub entry_end: String,
    pub flatten_at: String,
    pub expiry_day_only: bool,
    pub stale_after_ms: u32,
    pub max_spread_bps: u32,
    pub loss_streak_pause: u32,
    pub loss_pause_minutes: u32,
    pub margin_check: bool,
    pub limit_buffer_ticks: u32,
    pub market_protection_pct: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RejectionDto {
    #[ts(type = "number")]
    pub ts: i64,
    pub token: u32,
    pub strategy: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RiskUsageDto {
    #[ts(type = "number")]
    pub pnl: i64,
    #[ts(type = "number")]
    pub loss_used: i64,
    pub trades_today: u32,
    pub open_positions: u32,
    pub orders_last_sec: u32,
    pub orders_last_min: u32,
    pub loss_streak: u32,
    #[ts(type = "number | null")]
    pub paused_until: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RiskDto {
    pub limits: RiskLimitsDto,
    pub usage: RiskUsageDto,
    pub kill: KillDto,
    pub rejections: Vec<RejectionDto>,
    pub market_open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StrategyDto {
    pub id: String,
    pub enabled: bool,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StrategiesDto {
    pub strategies: Vec<StrategyDto>,
    pub ai_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AlertLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AlertDto {
    pub level: AlertLevel,
    /// For example `KillSwitch`, `StaleData`, `RiskRejected`, `BrokerRejected`.
    pub code: String,
    pub message: String,
    #[ts(type = "number")]
    pub ts: i64,
}
