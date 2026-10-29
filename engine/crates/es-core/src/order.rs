//! Order side, type, product and fills.

use crate::{Paise, Ts};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }

    /// +1 for buy, -1 for sell.
    pub fn sign(self) -> i64 {
        match self {
            Side::Buy => 1,
            Side::Sell => -1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    /// Market order. Always sent with market protection.
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Product {
    /// Intraday. The broker squares off if everything else fails.
    Mis,
    Nrml,
}

/// Engine-side order id. The broker id is tracked separately by the OMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub order: OrderId,
    pub token: u32,
    pub side: Side,
    /// Units, not lots.
    pub qty: u32,
    pub price: Paise,
    pub ts: Ts,
    pub strategy: String,
}
