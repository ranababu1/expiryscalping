//! Statutory charges and brokerage for index options, from a config table,
//! so paper P&L is net of costs. Rates are in parts per 100 million (1e-8):
//! 0.1% is 100_000. Values are config. Check them against Zerodha's charges
//! page before relying on them.

use es_core::{Paise, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChargesConfig {
    /// Flat brokerage per executed order.
    pub brokerage_per_order: Paise,
    /// STT on the sell side, on premium.
    pub stt_sell_e8: u64,
    /// Exchange transaction charges on premium, NSE (NFO).
    pub exchange_nse_e8: u64,
    /// Exchange transaction charges on premium, BSE (BFO).
    pub exchange_bse_e8: u64,
    /// SEBI turnover fee.
    pub sebi_e8: u64,
    /// Stamp duty on the buy side.
    pub stamp_buy_e8: u64,
    /// GST on brokerage, exchange charges and SEBI fee.
    pub gst_e8: u64,
}

impl Default for ChargesConfig {
    fn default() -> Self {
        ChargesConfig {
            brokerage_per_order: Paise::from_rupees(20),
            stt_sell_e8: 100_000,    // 0.1%
            exchange_nse_e8: 35_030, // 0.03503%
            exchange_bse_e8: 32_500, // 0.0325%
            sebi_e8: 100,            // Rs 10 per crore
            stamp_buy_e8: 3_000,     // 0.003%
            gst_e8: 18_000_000,      // 18%
        }
    }
}

impl ChargesConfig {
    /// Charges on one fill. `first_fill` is true for the first fill of an
    /// order, which carries the flat brokerage.
    pub fn on_fill(&self, side: Side, turnover: Paise, exchange: &str, first_fill: bool) -> Paise {
        let brokerage = if first_fill {
            self.brokerage_per_order
        } else {
            Paise::ZERO
        };
        let exch_rate = if exchange == "BFO" || exchange == "BSE" {
            self.exchange_bse_e8
        } else {
            self.exchange_nse_e8
        };
        let exchange_fee = turnover.apply_rate_e8(exch_rate);
        let sebi = turnover.apply_rate_e8(self.sebi_e8);
        let stt = match side {
            Side::Sell => turnover.apply_rate_e8(self.stt_sell_e8),
            Side::Buy => Paise::ZERO,
        };
        let stamp = match side {
            Side::Buy => turnover.apply_rate_e8(self.stamp_buy_e8),
            Side::Sell => Paise::ZERO,
        };
        let gst = (brokerage + exchange_fee + sebi).apply_rate_e8(self.gst_e8);
        brokerage + exchange_fee + sebi + stt + stamp + gst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_calculated_round_trip() {
        let c = ChargesConfig::default();
        // Buy 65 at Rs 100 = Rs 6,500 turnover (650,000 paise), NFO.
        // brokerage 2000, exchange 650000*0.0003503 = 227.695 -> 228,
        // sebi 0.65 -> 1, stamp 19.5 -> 20 (half up), gst 18% of 2229 = 401.22 -> 401
        let buy = c.on_fill(Side::Buy, Paise(650_000), "NFO", true);
        assert_eq!(buy, Paise(2_000 + 228 + 1 + 20 + 401));
        // Sell 65 at Rs 110 = 715,000 paise.
        // brokerage 2000, exchange 250.4645 -> 250, sebi 0.715 -> 1,
        // stt 715, gst 18% of 2251 = 405.18 -> 405
        let sell = c.on_fill(Side::Sell, Paise(715_000), "NFO", true);
        assert_eq!(sell, Paise(2_000 + 250 + 1 + 715 + 405));
    }

    #[test]
    fn later_fills_have_no_brokerage_and_bse_rate_differs() {
        let c = ChargesConfig::default();
        let a = c.on_fill(Side::Buy, Paise(100_000), "NFO", false);
        let b = c.on_fill(Side::Buy, Paise(100_000), "BFO", false);
        assert!(a < Paise(100));
        assert_ne!(a, b);
    }
}
