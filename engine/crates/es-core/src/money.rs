//! Integer money. One rupee is 100 paise. Never use floating point for money.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// An amount of money in paise. Signed so it can hold P&L.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Paise(pub i64);

impl Paise {
    pub const ZERO: Paise = Paise(0);

    pub const fn from_rupees(rupees: i64) -> Self {
        Paise(rupees * 100)
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    /// Price times a quantity in units (not lots).
    pub const fn times(self, qty: i64) -> Paise {
        Paise(self.0 * qty)
    }

    /// Applies a rate expressed in parts per 100 million (1e-8), rounded to
    /// the nearest paisa. 0.1% is 100_000, 18% is 18_000_000.
    pub fn apply_rate_e8(self, rate_e8: u64) -> Paise {
        let v = self.0 as i128 * rate_e8 as i128;
        let q = 100_000_000i128;
        let rounded = if v >= 0 { (v + q / 2) / q } else { (v - q / 2) / q };
        Paise(rounded as i64)
    }

    pub const fn abs(self) -> Paise {
        Paise(self.0.abs())
    }
}

impl Add for Paise {
    type Output = Paise;
    fn add(self, rhs: Paise) -> Paise {
        Paise(self.0 + rhs.0)
    }
}

impl AddAssign for Paise {
    fn add_assign(&mut self, rhs: Paise) {
        self.0 += rhs.0;
    }
}

impl Sub for Paise {
    type Output = Paise;
    fn sub(self, rhs: Paise) -> Paise {
        Paise(self.0 - rhs.0)
    }
}

impl SubAssign for Paise {
    fn sub_assign(&mut self, rhs: Paise) {
        self.0 -= rhs.0;
    }
}

impl Neg for Paise {
    type Output = Paise;
    fn neg(self) -> Paise {
        Paise(-self.0)
    }
}

impl fmt::Display for Paise {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.unsigned_abs();
        write!(f, "{sign}Rs {}.{:02}", a / 100, a % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_rounding() {
        // 0.1% of Rs 1,000.00 = Rs 1.00
        assert_eq!(Paise(100_000).apply_rate_e8(100_000), Paise(100));
        // 18% of 2,000 paise = 360 paise
        assert_eq!(Paise(2_000).apply_rate_e8(18_000_000), Paise(360));
        // rounds half up: 0.003% of 50,000 paise = 1.5 -> 2
        assert_eq!(Paise(50_000).apply_rate_e8(3_000), Paise(2));
    }

    #[test]
    fn display() {
        assert_eq!(Paise(-12_345).to_string(), "-Rs 123.45");
        assert_eq!(Paise::from_rupees(2_000).to_string(), "Rs 2000.00");
    }
}
