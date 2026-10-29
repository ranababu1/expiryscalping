//! Positions with average cost, realised and unrealised P&L net of charges.

use es_core::{Paise, Side};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Position {
    pub token: u32,
    /// Net quantity in units (positive long).
    pub qty: i64,
    /// Average entry price of the open quantity.
    pub avg_price: Paise,
    /// Realised P&L before charges, all day.
    pub realised_gross: Paise,
    /// Charges paid on this instrument, all day.
    pub charges: Paise,
    /// Realised P&L net of charges since the position last opened from flat.
    pub trade_pnl: Paise,
    pub last_price: Paise,
}

impl Position {
    pub fn new(token: u32) -> Position {
        Position {
            token,
            ..Position::default()
        }
    }

    pub fn realised(&self) -> Paise {
        self.realised_gross - self.charges
    }

    pub fn unrealised(&self, mark: Paise) -> Paise {
        if self.qty == 0 {
            return Paise::ZERO;
        }
        (mark - self.avg_price).times(self.qty)
    }

    /// Applies a fill. Returns `Some(trade_pnl)` when the position goes flat.
    pub fn apply(&mut self, side: Side, qty: u32, price: Paise, charges: Paise) -> Option<Paise> {
        let signed = side.sign() * qty as i64;
        let was_flat = self.qty == 0;
        if was_flat {
            self.trade_pnl = Paise::ZERO;
        }
        self.charges += charges;
        self.trade_pnl -= charges;
        self.last_price = price;
        if self.qty == 0 || self.qty.signum() == signed.signum() {
            // Opening or adding: weighted average, rounded to the paisa.
            let total = self.qty + signed;
            let cost =
                self.avg_price.0 as i128 * self.qty.abs() as i128 + price.0 as i128 * qty as i128;
            self.avg_price = Paise((cost / total.abs() as i128) as i64);
            self.qty = total;
        } else {
            // Reducing, closing or flipping.
            let closing = signed.abs().min(self.qty.abs());
            let pnl = (price - self.avg_price).times(closing * self.qty.signum());
            self.realised_gross += pnl;
            self.trade_pnl += pnl;
            let rest = signed.abs() - closing;
            self.qty += signed.signum() * closing;
            if rest > 0 {
                self.qty = signed.signum() * rest;
                self.avg_price = price;
            }
        }
        if self.qty == 0 && !was_flat {
            self.avg_price = Paise::ZERO;
            return Some(self.trade_pnl);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_charges() {
        let mut p = Position::new(1);
        assert_eq!(p.apply(Side::Buy, 65, Paise(10_000), Paise(100)), None);
        assert_eq!(p.apply(Side::Buy, 65, Paise(10_100), Paise(100)), None);
        assert_eq!((p.qty, p.avg_price), (130, Paise(10_050)));
        assert_eq!(p.unrealised(Paise(10_150)), Paise(13_000));
        assert_eq!(p.apply(Side::Sell, 65, Paise(10_150), Paise(100)), None);
        assert_eq!(p.realised_gross, Paise(6_500));
        let closed = p.apply(Side::Sell, 65, Paise(10_050), Paise(100));
        assert_eq!(closed, Some(Paise(6_500 - 400)));
        assert_eq!(p.realised(), Paise(6_100));
        assert_eq!((p.qty, p.avg_price), (0, Paise::ZERO));
        // Next trade's P&L starts from zero.
        p.apply(Side::Buy, 65, Paise(10_000), Paise(0));
        assert_eq!(
            p.apply(Side::Sell, 65, Paise(9_900), Paise(0)),
            Some(Paise(-6_500))
        );
        assert_eq!(p.realised(), Paise(-400));
    }

    #[test]
    fn flip_through_zero() {
        let mut p = Position::new(1);
        p.apply(Side::Buy, 10, Paise(100), Paise::ZERO);
        p.apply(Side::Sell, 15, Paise(110), Paise::ZERO);
        assert_eq!(
            (p.qty, p.avg_price, p.realised_gross),
            (-5, Paise(110), Paise(100))
        );
    }
}
