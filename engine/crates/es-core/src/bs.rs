//! Black-Scholes pricing and implied volatility for European index options.
//! Floating point is fine here: this is a model value, never a money amount
//! that gets booked. Rate and dividends are taken as zero (expiry day).

/// Standard normal CDF (Abramowitz and Stegun 7.1.26, error under 1e-7).
pub fn norm_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.231_641_9 * x.abs());
    let poly = t
        * (0.319_381_530
            + t * (-0.356_563_782 + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let pdf = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let upper = pdf * poly;
    if x >= 0.0 { 1.0 - upper } else { upper }
}

/// Option price. `t_years` is time to expiry in years, `vol` annualised.
pub fn price(is_call: bool, spot: f64, strike: f64, t_years: f64, vol: f64) -> f64 {
    let intrinsic = if is_call {
        (spot - strike).max(0.0)
    } else {
        (strike - spot).max(0.0)
    };
    if t_years <= 0.0 || vol <= 0.0 || spot <= 0.0 || strike <= 0.0 {
        return intrinsic;
    }
    let sd = vol * t_years.sqrt();
    let d1 = ((spot / strike).ln() + 0.5 * sd * sd) / sd;
    let d2 = d1 - sd;
    if is_call {
        spot * norm_cdf(d1) - strike * norm_cdf(d2)
    } else {
        strike * norm_cdf(-d2) - spot * norm_cdf(-d1)
    }
}

/// Implied volatility by bisection. `None` if the price is below intrinsic
/// or no volatility under 500% matches.
pub fn implied_vol(is_call: bool, premium: f64, spot: f64, strike: f64, t_years: f64) -> Option<f64> {
    if t_years <= 0.0 || premium <= 0.0 {
        return None;
    }
    let (mut lo, mut hi) = (1e-4, 5.0);
    if premium < price(is_call, spot, strike, t_years, lo) || premium > price(is_call, spot, strike, t_years, hi) {
        return None;
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if price(is_call, spot, strike, t_years, mid) > premium {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_call_parity_and_iv_round_trip() {
        let (s, k, t, v) = (24_050.0, 24_000.0, 0.5 / 365.0, 0.13);
        let c = price(true, s, k, t, v);
        let p = price(false, s, k, t, v);
        assert!((c - p - (s - k)).abs() < 1e-6);
        let iv = implied_vol(true, c, s, k, t).unwrap();
        assert!((iv - v).abs() < 1e-4, "{iv}");
    }

    #[test]
    fn expired_is_intrinsic() {
        assert_eq!(price(true, 100.0, 90.0, 0.0, 0.2), 10.0);
        assert_eq!(price(false, 100.0, 90.0, 0.0, 0.2), 0.0);
        assert!(implied_vol(true, 1.0, 100.0, 90.0, 0.01).is_none());
    }
}
