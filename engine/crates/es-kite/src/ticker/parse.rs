use super::{LEN_FULL, LEN_INDEX_FULL, LEN_INDEX_QUOTE, LEN_LTP, LEN_QUOTE, Segment};
use es_core::{Depth5, DepthLevel, Paise, Tick, Ts};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("frame is empty")]
    Empty,
    #[error("frame truncated at byte {at} (need {need} more)")]
    Truncated { at: usize, need: usize },
    #[error("frame has {extra} trailing bytes")]
    Trailing { extra: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Heartbeat,
    /// A data frame; `ticks` were appended, `skipped` packets were ignored
    /// (unknown length or unsupported segment).
    Data { ticks: usize, skipped: usize },
}

/// Allocation-free parser. The caller owns and reuses the output vector.
#[derive(Debug, Default)]
pub struct Parser;

fn be_u16(b: &[u8], at: usize) -> Result<u16, ParseError> {
    b.get(at..at + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or(ParseError::Truncated { at, need: 2 })
}

/// Reads a big-endian i32 from a packet already checked for length.
fn i32_at(p: &[u8], at: usize) -> i32 {
    i32::from_be_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]])
}

fn u32_at(p: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]])
}

fn price(p: &[u8], at: usize) -> Paise {
    Paise(i32_at(p, at) as i64)
}

fn nonneg(v: i32) -> u32 {
    v.max(0) as u32
}

impl Parser {
    /// Parses one binary WebSocket frame, appending ticks to `out`.
    pub fn parse_frame(
        &self,
        frame: &[u8],
        recv: Ts,
        out: &mut Vec<Tick>,
    ) -> Result<FrameKind, ParseError> {
        match frame.len() {
            0 => return Err(ParseError::Empty),
            1 => return Ok(FrameKind::Heartbeat),
            _ => {}
        }
        let count = be_u16(frame, 0)? as usize;
        let mut at = 2;
        let start_len = out.len();
        let mut skipped = 0;
        for _ in 0..count {
            let len = be_u16(frame, at)? as usize;
            at += 2;
            let packet = frame.get(at..at + len).ok_or(ParseError::Truncated {
                at,
                need: len.saturating_sub(frame.len().saturating_sub(at)),
            })?;
            at += len;
            match parse_packet(packet, recv) {
                Some(t) => out.push(t),
                None => skipped += 1,
            }
        }
        if at != frame.len() {
            out.truncate(start_len);
            return Err(ParseError::Trailing {
                extra: frame.len() - at,
            });
        }
        Ok(FrameKind::Data {
            ticks: out.len() - start_len,
            skipped,
        })
    }
}

fn parse_packet(p: &[u8], recv: Ts) -> Option<Tick> {
    if p.len() < LEN_LTP {
        return None;
    }
    let token = u32_at(p, 0);
    let segment = Segment::of_token(token)?;
    if !segment.prices_in_paise() {
        return None;
    }
    let mut t = Tick {
        token,
        ltp: price(p, 4),
        recv,
        ..Tick::default()
    };
    let is_index = segment == Segment::Indices;
    match (p.len(), is_index) {
        (LEN_LTP, _) => {}
        (LEN_INDEX_QUOTE, true) | (LEN_INDEX_FULL, true) => {
            t.close = price(p, 20);
            if p.len() == LEN_INDEX_FULL {
                t.exchange_ts = i32_at(p, 28) as i64;
            }
        }
        (LEN_QUOTE, false) | (LEN_FULL, false) => {
            t.ltq = nonneg(i32_at(p, 8));
            t.volume = nonneg(i32_at(p, 16)) as u64;
            t.close = price(p, 40);
            if p.len() == LEN_FULL {
                t.oi = nonneg(i32_at(p, 48)) as u64;
                t.exchange_ts = i32_at(p, 60) as i64;
                t.depth = Some(parse_depth(&p[64..LEN_FULL]));
            }
        }
        _ => return None,
    }
    Some(t)
}

fn parse_depth(d: &[u8]) -> Depth5 {
    let level = |i: usize| {
        let o = i * 12;
        DepthLevel {
            qty: nonneg(i32_at(d, o)),
            price: price(d, o + 4),
            orders: u16::from_be_bytes([d[o + 8], d[o + 9]]),
        }
    };
    let mut depth = Depth5::default();
    for i in 0..5 {
        depth.bids[i] = level(i);
        depth.asks[i] = level(i + 5);
    }
    depth
}
