//! The Kite instrument master CSV (`GET /instruments`).
//!
//! Columns: `instrument_token, exchange_token, tradingsymbol, name,
//! last_price, expiry, strike, tick_size, lot_size, instrument_type,
//! segment, exchange`.

use es_core::{Date, Instrument, InstrumentKind, Paise};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstrumentsError {
    #[error("missing header row")]
    MissingHeader,
    #[error("missing column {0}")]
    MissingColumn(&'static str),
    #[error("line {line}: {msg}")]
    BadRow { line: usize, msg: String },
}

pub const HEADER: &str = "instrument_token,exchange_token,tradingsymbol,name,last_price,expiry,strike,tick_size,lot_size,instrument_type,segment,exchange";

/// Splits one CSV line, honouring double quotes.
fn split_csv(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Parses a decimal rupee string such as `24000.0` or `0.05` into paise
/// without floating point.
pub fn parse_rupees(s: &str) -> Option<Paise> {
    let s = s.trim();
    if s.is_empty() {
        return Some(Paise::ZERO);
    }
    let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let mut frac2 = String::from(frac);
    if !frac2.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    frac2.truncate(2);
    while frac2.len() < 2 {
        frac2.push('0');
    }
    let p = whole * 100 + frac2.parse::<i64>().ok()?;
    Some(Paise(if neg { -p } else { p }))
}

/// Parses the CSV into instruments. Rows of unsupported types (equities,
/// currencies) are skipped. Index rows (segment `INDICES`) are kept.
pub fn parse_csv(csv: &str) -> Result<Vec<Instrument>, InstrumentsError> {
    let mut lines = csv.lines().enumerate();
    let (_, header) = lines.next().ok_or(InstrumentsError::MissingHeader)?;
    let cols = split_csv(header);
    let idx = |name: &'static str| {
        cols.iter()
            .position(|c| c.trim() == name)
            .ok_or(InstrumentsError::MissingColumn(name))
    };
    let c_token = idx("instrument_token")?;
    let c_sym = idx("tradingsymbol")?;
    let c_name = idx("name")?;
    let c_expiry = idx("expiry")?;
    let c_strike = idx("strike")?;
    let c_tick = idx("tick_size")?;
    let c_lot = idx("lot_size")?;
    let c_type = idx("instrument_type")?;
    let c_seg = idx("segment")?;
    let c_exch = idx("exchange")?;

    let mut out = Vec::new();
    for (i, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 1;
        let bad = |msg: &str| InstrumentsError::BadRow {
            line: line_no,
            msg: msg.to_string(),
        };
        let f = split_csv(line);
        let get = |c: usize| f.get(c).map(|s| s.trim()).unwrap_or("");
        let kind = match (get(c_type), get(c_seg)) {
            (_, "INDICES") => InstrumentKind::Index,
            ("CE", _) => InstrumentKind::Ce,
            ("PE", _) => InstrumentKind::Pe,
            ("FUT", _) => InstrumentKind::Fut,
            _ => continue,
        };
        let token: u32 = get(c_token).parse().map_err(|_| bad("bad instrument_token"))?;
        let expiry = match get(c_expiry) {
            "" => None,
            s => Some(Date::parse(s).map_err(|_| bad("bad expiry"))?),
        };
        let strike = parse_rupees(get(c_strike)).ok_or_else(|| bad("bad strike"))?;
        let tick_size = parse_rupees(get(c_tick)).ok_or_else(|| bad("bad tick_size"))?;
        let lot_size: u32 = match get(c_lot) {
            "" => 0,
            s => s.parse().map_err(|_| bad("bad lot_size"))?,
        };
        out.push(Instrument {
            token,
            tradingsymbol: get(c_sym).to_string(),
            underlying: get(c_name).to_string(),
            exchange: get(c_exch).to_string(),
            kind,
            expiry,
            strike,
            tick_size,
            lot_size,
        });
    }
    Ok(out)
}

/// Writes instruments in the same CSV format (used by the simulator).
pub fn to_csv(instruments: &[Instrument]) -> String {
    let mut s = String::from(HEADER);
    s.push('\n');
    for i in instruments {
        let (itype, seg) = match i.kind {
            InstrumentKind::Index => ("EQ", "INDICES".to_string()),
            InstrumentKind::Ce => ("CE", format!("{}-OPT", opt_segment(&i.exchange))),
            InstrumentKind::Pe => ("PE", format!("{}-OPT", opt_segment(&i.exchange))),
            InstrumentKind::Fut => ("FUT", format!("{}-FUT", opt_segment(&i.exchange))),
        };
        let rupees = |p: Paise| format!("{}.{:02}", p.0 / 100, p.0 % 100);
        s.push_str(&format!(
            "{},{},{},\"{}\",0,{},{},{},{},{},{},{}\n",
            i.token,
            i.token >> 8,
            i.tradingsymbol,
            i.underlying,
            i.expiry.map(|d| d.to_string()).unwrap_or_default(),
            rupees(i.strike),
            rupees(i.tick_size),
            i.lot_size,
            itype,
            seg,
            i.exchange
        ));
    }
    s
}

fn opt_segment(exchange: &str) -> &str {
    match exchange {
        "BFO" => "BFO",
        _ => "NFO",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "instrument_token,exchange_token,tradingsymbol,name,last_price,expiry,strike,tick_size,lot_size,instrument_type,segment,exchange
256265,1001,NIFTY 50,\"NIFTY 50\",0,,0,0,0,EQ,INDICES,NSE
12345602,48225,NIFTY26N0324000CE,\"NIFTY\",0,2026-11-03,24000.0,0.05,65,CE,NFO-OPT,NFO
12345702,48226,NIFTY26N0324000PE,\"NIFTY\",0,2026-11-03,24000.0,0.05,65,PE,NFO-OPT,NFO
408065,1594,INFY,\"INFOSYS\",0,,0,0.05,1,EQ,NSE,NSE
";

    #[test]
    fn parses_master() {
        let v = parse_csv(SAMPLE).unwrap();
        assert_eq!(v.len(), 3, "equity row skipped");
        assert_eq!(v[0].kind, InstrumentKind::Index);
        assert_eq!(v[1].kind, InstrumentKind::Ce);
        assert_eq!(v[1].strike, Paise(2_400_000));
        assert_eq!(v[1].tick_size, Paise(5));
        assert_eq!(v[1].lot_size, 65);
        assert_eq!(v[1].underlying, "NIFTY");
        assert_eq!(v[1].expiry, Some(Date::new(2026, 11, 3).unwrap()));
    }

    #[test]
    fn round_trip() {
        let v = parse_csv(SAMPLE).unwrap();
        let again = parse_csv(&to_csv(&v)).unwrap();
        assert_eq!(v, again);
    }

    #[test]
    fn rupee_parsing() {
        assert_eq!(parse_rupees("0.05"), Some(Paise(5)));
        assert_eq!(parse_rupees("24000"), Some(Paise(2_400_000)));
        assert_eq!(parse_rupees("1.5"), Some(Paise(150)));
        assert_eq!(parse_rupees("-2.25"), Some(Paise(-225)));
        assert_eq!(parse_rupees("x"), None);
    }

    #[test]
    fn bad_rows_report_line() {
        let csv = format!("{HEADER}\nabc,1,X,\"NIFTY\",0,2026-11-03,1,0.05,65,CE,NFO-OPT,NFO\n");
        assert!(matches!(
            parse_csv(&csv),
            Err(InstrumentsError::BadRow { line: 2, .. })
        ));
    }
}
