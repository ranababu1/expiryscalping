//! Risk limits, set by Imran. Defaults live in `engine/config/risk.toml`.

use es_core::Paise;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A wall time in IST, minutes since midnight. Written as `"HH:MM"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HhMm(pub u32);

impl HhMm {
    pub fn parse(s: &str) -> Option<HhMm> {
        let (h, m) = s.trim().split_once(':')?;
        let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(HhMm(h * 60 + m))
    }

    pub fn hour(self) -> u32 {
        self.0 / 60
    }

    pub fn minute(self) -> u32 {
        self.0 % 60
    }
}

impl std::fmt::Display for HhMm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour(), self.minute())
    }
}

impl Serialize for HhMm {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HhMm {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<HhMm, D::Error> {
        let s = String::deserialize(d)?;
        HhMm::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("bad time {s:?}, expected HH:MM")))
    }
}

/// Exchange session hours (market facts, from config).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketHours {
    pub open: HhMm,
    pub close: HhMm,
}

impl Default for MarketHours {
    fn default() -> Self {
        MarketHours {
            open: HhMm(9 * 60 + 15),
            close: HhMm(15 * 60 + 30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskLimits {
    /// Loss (realised plus unrealised, net of charges) that triggers the kill
    /// switch for the rest of the day. A positive amount in paise.
    pub daily_max_loss: Paise,
    /// Max premium times quantity for one entry, in paise.
    pub per_trade_max_premium: Paise,
    /// Max lots per entry.
    pub per_trade_max_lots: u32,
    /// Max concurrent open positions across all strategies.
    pub max_open_positions: u32,
    /// Max entries per day.
    pub max_trades_per_day: u32,
    /// Our own order rate cap, well below Kite's limits.
    pub max_orders_per_sec: u32,
    pub max_orders_per_min: u32,
    /// Entries allowed from `entry_start` (inclusive) to `entry_end` (exclusive).
    pub entry_start: HhMm,
    pub entry_end: HhMm,
    /// Everything is flattened at this time.
    pub flatten_at: HhMm,
    /// Entries only on instruments that expire today.
    pub expiry_day_only: bool,
    /// No entries if the instrument's last tick is older than this.
    pub stale_after_ms: u32,
    /// No entries if the bid-ask spread is wider than this, in basis points
    /// of the mid price (200 = 2%).
    pub max_spread_bps: u32,
    /// After this many losing trades in a row, pause entries.
    pub loss_streak_pause: u32,
    pub loss_pause_minutes: u32,
    /// Required premium must fit available funds.
    pub margin_check: bool,
    /// Ticks added beyond the best opposite price for marketable limit orders.
    pub limit_buffer_ticks: u32,
    /// Market protection percent for market orders (kill switch exits).
    pub market_protection_pct: u32,
}

impl Default for RiskLimits {
    /// The plan's suggested defaults for about Rs 1 lakh of capital.
    fn default() -> Self {
        RiskLimits {
            daily_max_loss: Paise::from_rupees(2_000),
            per_trade_max_premium: Paise::from_rupees(15_000),
            per_trade_max_lots: 1,
            max_open_positions: 2,
            max_trades_per_day: 20,
            max_orders_per_sec: 2,
            max_orders_per_min: 60,
            entry_start: HhMm(9 * 60 + 20),
            entry_end: HhMm(15 * 60),
            flatten_at: HhMm(15 * 60 + 15),
            expiry_day_only: true,
            stale_after_ms: 1_500,
            max_spread_bps: 200,
            loss_streak_pause: 3,
            loss_pause_minutes: 15,
            margin_check: true,
            limit_buffer_ticks: 2,
            market_protection_pct: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LimitsError {
    #[error("{0} must be greater than zero")]
    MustBePositive(&'static str),
    #[error("entry_start must be before entry_end, and entry_end at or before flatten_at")]
    BadWindow,
    #[error(
        "max_orders_per_sec must not exceed max_orders_per_min, and both must stay at or below 10 per second"
    )]
    BadRate,
    #[error("market_protection_pct must be between 1 and 20")]
    BadProtection,
    #[error(
        "during market hours limits can only be tightened, unless the engine is flat and you confirm"
    )]
    LooserNeedsFlatAndConfirm,
}

impl RiskLimits {
    pub fn validate(&self) -> Result<(), LimitsError> {
        let pos = |v: u64, name| {
            if v == 0 {
                Err(LimitsError::MustBePositive(name))
            } else {
                Ok(())
            }
        };
        pos(self.daily_max_loss.0.max(0) as u64, "daily_max_loss")?;
        pos(
            self.per_trade_max_premium.0.max(0) as u64,
            "per_trade_max_premium",
        )?;
        pos(self.per_trade_max_lots as u64, "per_trade_max_lots")?;
        pos(self.max_open_positions as u64, "max_open_positions")?;
        pos(self.max_trades_per_day as u64, "max_trades_per_day")?;
        pos(self.max_orders_per_sec as u64, "max_orders_per_sec")?;
        pos(self.max_orders_per_min as u64, "max_orders_per_min")?;
        pos(self.stale_after_ms as u64, "stale_after_ms")?;
        pos(self.max_spread_bps as u64, "max_spread_bps")?;
        pos(self.loss_streak_pause as u64, "loss_streak_pause")?;
        if !(self.entry_start < self.entry_end && self.entry_end <= self.flatten_at) {
            return Err(LimitsError::BadWindow);
        }
        if self.max_orders_per_sec > 10 || self.max_orders_per_sec > self.max_orders_per_min {
            return Err(LimitsError::BadRate);
        }
        if !(1..=20).contains(&self.market_protection_pct) {
            return Err(LimitsError::BadProtection);
        }
        Ok(())
    }

    /// True if `new` is at least as strict as `self` on every rule.
    pub fn is_tightened_by(&self, new: &RiskLimits) -> bool {
        new.daily_max_loss <= self.daily_max_loss
            && new.per_trade_max_premium <= self.per_trade_max_premium
            && new.per_trade_max_lots <= self.per_trade_max_lots
            && new.max_open_positions <= self.max_open_positions
            && new.max_trades_per_day <= self.max_trades_per_day
            && new.max_orders_per_sec <= self.max_orders_per_sec
            && new.max_orders_per_min <= self.max_orders_per_min
            && new.entry_start >= self.entry_start
            && new.entry_end <= self.entry_end
            && new.flatten_at <= self.flatten_at
            && (new.expiry_day_only || !self.expiry_day_only)
            && new.stale_after_ms <= self.stale_after_ms
            && new.max_spread_bps <= self.max_spread_bps
            && new.loss_streak_pause <= self.loss_streak_pause
            && new.loss_pause_minutes >= self.loss_pause_minutes
            && (new.margin_check || !self.margin_check)
            && new.limit_buffer_ticks <= self.limit_buffer_ticks
            && new.market_protection_pct <= self.market_protection_pct
    }
}
