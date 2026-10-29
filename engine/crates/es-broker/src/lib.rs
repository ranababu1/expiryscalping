//! The broker boundary. The order manager talks to a [`Broker`], which is
//! either the [`paper::PaperBroker`] (default) or the Kite broker (Phase 3).
//! Both take only risk-approved [`es_risk::OrderRequest`]s.

pub mod charges;
pub mod kite;
pub mod paper;

pub use charges::ChargesConfig;
pub use kite::KiteBroker;
pub use paper::{PaperBroker, PaperConfig};

use async_trait::async_trait;
use es_core::{OrderType, Paise, Side, Ts};
use es_risk::{ApprovedModify, OrderRequest};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct BrokerOrderId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrokerError {
    #[error("order rejected: {0}")]
    Rejected(String),
    #[error("unknown order {0}")]
    UnknownOrder(String),
    #[error("order is not open")]
    NotOpen,
    #[error("network or broker error, order state unknown: {0}")]
    Unknown(String),
    #[error("session expired, log in to Kite")]
    SessionExpired,
    #[error("not available: {0}")]
    NotAvailable(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BrokerOrderStatus {
    Open,
    Complete,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokerOrder {
    pub id: BrokerOrderId,
    /// Our tag, used to match orders whose state is unknown.
    pub tag: String,
    pub token: u32,
    pub side: Side,
    pub qty: u32,
    pub filled: u32,
    pub limit: Option<Paise>,
    pub order_type: OrderType,
    pub status: BrokerOrderStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Position {
    pub token: u32,
    pub qty: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Margins {
    pub available: Paise,
}

/// Order updates from the broker (postbacks for Kite, simulated for paper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerEvent {
    /// One execution. `trade_id` is unique per execution, so a duplicate
    /// postback can be detected.
    Fill {
        id: BrokerOrderId,
        trade_id: String,
        qty: u32,
        price: Paise,
        ts: Ts,
    },
    Complete {
        id: BrokerOrderId,
    },
    Cancelled {
        id: BrokerOrderId,
    },
    Rejected {
        id: BrokerOrderId,
        reason: String,
    },
    Modified {
        id: BrokerOrderId,
        qty: u32,
        limit: Paise,
    },
}

#[async_trait]
pub trait Broker: Send + Sync {
    async fn place(&self, req: &OrderRequest, tag: &str) -> Result<BrokerOrderId, BrokerError>;
    async fn modify(&self, id: &BrokerOrderId, m: ApprovedModify) -> Result<(), BrokerError>;
    async fn cancel(&self, id: &BrokerOrderId) -> Result<(), BrokerError>;
    async fn orders(&self) -> Result<Vec<BrokerOrder>, BrokerError>;
    async fn positions(&self) -> Result<Vec<Position>, BrokerError>;
    async fn margins(&self) -> Result<Margins, BrokerError>;
}
