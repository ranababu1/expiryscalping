use es_core::{Date, OrderType, Paise, Product, ReasonCode, Side, Ts};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RejectReason {
    KillSwitch,
    DailyMaxLoss,
    OutsideEntryWindow,
    NotExpiryDay,
    StaleData,
    NoDepth,
    SpreadTooWide,
    LossStreakPause,
    MaxTradesPerDay,
    MaxOpenPositions,
    RateLimited,
    PremiumTooHigh,
    InsufficientMargin,
    ShortEntryNotAllowed,
    UnknownInstrument,
    NoPosition,
    ExitAlreadyPending,
    ZeroQuantity,
    /// Used with `Resize`: lots were cut to fit per-trade size.
    ResizedToMaxLots,
    /// Used with `Resize`: lots were cut to fit per-trade premium.
    ResizedToMaxPremium,
    /// Used with `Resize`: lots were cut to fit available funds.
    ResizedToMargin,
}

impl RejectReason {
    pub fn message(self) -> &'static str {
        use RejectReason::*;
        match self {
            KillSwitch => "Kill switch is on. Re-arm it to allow entries.",
            DailyMaxLoss => "Daily max loss reached. No entries for the rest of the day.",
            OutsideEntryWindow => "Outside the entry window.",
            NotExpiryDay => "Instrument does not expire today.",
            StaleData => "Market data is stale.",
            NoDepth => "No bid or offer in the order book.",
            SpreadTooWide => "Bid-ask spread is too wide.",
            LossStreakPause => "Paused after consecutive losses.",
            MaxTradesPerDay => "Max trades per day reached.",
            MaxOpenPositions => "Max open positions reached.",
            RateLimited => "Order rate cap reached.",
            PremiumTooHigh => "Premium is above the per-trade limit, even for one lot.",
            InsufficientMargin => "Not enough funds for one lot.",
            ShortEntryNotAllowed => "Selling to open is not allowed. Entries are option buys.",
            UnknownInstrument => "Unknown instrument.",
            NoPosition => "No open position to exit.",
            ExitAlreadyPending => "An exit order is already working for this position.",
            ZeroQuantity => "Quantity must be at least one lot.",
            ResizedToMaxLots => "Resized to the per-trade max lots.",
            ResizedToMaxPremium => "Resized to the per-trade max premium.",
            ResizedToMargin => "Resized to available funds.",
        }
    }
}

/// An order approved by the risk engine. Only `es-risk` can build one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderRequest {
    token: u32,
    side: Side,
    qty: u32,
    lots: u32,
    order_type: OrderType,
    limit: Option<Paise>,
    product: Product,
    market_protection_pct: u32,
    is_exit: bool,
    strategy: &'static str,
    reason: ReasonCode,
    approved_at: Ts,
}

impl OrderRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        token: u32,
        side: Side,
        lots: u32,
        lot_size: u32,
        order_type: OrderType,
        limit: Option<Paise>,
        market_protection_pct: u32,
        is_exit: bool,
        strategy: &'static str,
        reason: ReasonCode,
        approved_at: Ts,
    ) -> OrderRequest {
        OrderRequest {
            token,
            side,
            qty: lots * lot_size,
            lots,
            order_type,
            limit,
            product: Product::Mis,
            market_protection_pct,
            is_exit,
            strategy,
            reason,
            approved_at,
        }
    }

    pub fn token(&self) -> u32 {
        self.token
    }
    pub fn side(&self) -> Side {
        self.side
    }
    /// Quantity in units.
    pub fn qty(&self) -> u32 {
        self.qty
    }
    pub fn lots(&self) -> u32 {
        self.lots
    }
    pub fn order_type(&self) -> OrderType {
        self.order_type
    }
    pub fn limit(&self) -> Option<Paise> {
        self.limit
    }
    pub fn product(&self) -> Product {
        self.product
    }
    pub fn market_protection_pct(&self) -> u32 {
        self.market_protection_pct
    }
    pub fn is_exit(&self) -> bool {
        self.is_exit
    }
    pub fn strategy(&self) -> &'static str {
        self.strategy
    }
    pub fn reason(&self) -> ReasonCode {
        self.reason
    }
    pub fn approved_at(&self) -> Ts {
        self.approved_at
    }
}

/// A modification approved by the risk engine. Only `es-risk` can build one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ApprovedModify {
    qty: u32,
    limit: Paise,
}

impl ApprovedModify {
    pub(crate) fn new(qty: u32, limit: Paise) -> ApprovedModify {
        ApprovedModify { qty, limit }
    }
    pub fn qty(&self) -> u32 {
        self.qty
    }
    pub fn limit(&self) -> Paise {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RiskDecision {
    Approve(OrderRequest),
    /// Approved with a smaller quantity that fits the limits.
    Resize(OrderRequest, RejectReason),
    Reject(RejectReason),
}

impl RiskDecision {
    pub fn order(self) -> Option<OrderRequest> {
        match self {
            RiskDecision::Approve(o) | RiskDecision::Resize(o, _) => Some(o),
            RiskDecision::Reject(_) => None,
        }
    }
}

/// What the risk engine needs to know about the instrument being traded.
/// The engine builds this from market state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstrumentView {
    pub token: u32,
    pub lot_size: u32,
    pub tick_size: Paise,
    pub expiry: Option<Date>,
    pub best_bid: Option<Paise>,
    pub best_ask: Option<Paise>,
    pub ltp: Paise,
    /// Milliseconds since the last tick (`None` if never ticked).
    pub age_ms: Option<i64>,
}

impl InstrumentView {
    pub fn spread_bps(&self) -> Option<u32> {
        let (b, a) = (self.best_bid?, self.best_ask?);
        let mid = (a.0 + b.0) / 2;
        (mid > 0 && a >= b).then(|| ((a.0 - b.0) * 10_000 / mid) as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionView {
    pub token: u32,
    /// Net quantity in units (positive long, negative short).
    pub qty: i64,
    /// Units already covered by working exit orders.
    pub pending_exit_qty: u32,
}

/// Portfolio facts the risk engine reads. Built by the order manager.
#[derive(Debug, Clone, Copy)]
pub struct PortfolioView<'a> {
    /// Realised P&L net of charges.
    pub realised: Paise,
    pub unrealised: Paise,
    pub positions: &'a [PositionView],
    /// Tokens of working entry orders with no position yet.
    pub pending_entry_tokens: &'a [u32],
    pub available_funds: Paise,
    /// True when the feed is disconnected or reconnecting.
    pub feed_stale: bool,
}

impl PortfolioView<'_> {
    pub fn total_pnl(&self) -> Paise {
        self.realised + self.unrealised
    }

    pub fn position(&self, token: u32) -> Option<&PositionView> {
        self.positions
            .iter()
            .find(|p| p.token == token && p.qty != 0)
    }

    /// Open positions plus working entries on instruments with no position.
    pub fn open_count(&self) -> u32 {
        let open = self.positions.iter().filter(|p| p.qty != 0).count();
        let pending = self
            .pending_entry_tokens
            .iter()
            .filter(|t| self.position(**t).is_none())
            .count();
        (open + pending) as u32
    }

    pub fn is_flat(&self) -> bool {
        self.positions.iter().all(|p| p.qty == 0) && self.pending_entry_tokens.is_empty()
    }
}

/// A working order, as seen by `check_modify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOrderView {
    pub is_exit: bool,
    pub side: Side,
    pub lot_size: u32,
}
