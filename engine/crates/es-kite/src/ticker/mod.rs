//! Kite WebSocket binary frames.
//!
//! A frame is: `u16 packet count`, then for each packet `u16 length` and the
//! packet bytes. All integers are big-endian. A frame of exactly one byte is
//! a heartbeat. Packet layout is chosen by length:
//!
//! | Length | Packet |
//! |---|---|
//! | 8 | LTP mode (token, last price) |
//! | 28 | Index quote |
//! | 32 | Index full (quote plus exchange timestamp) |
//! | 44 | Quote mode |
//! | 184 | Full mode (quote, OI, timestamps, 5-level depth) |

mod encode;
mod parse;

pub use encode::{FrameBuilder, FullPacket, IndexPacket};
pub use parse::{FrameKind, ParseError, Parser};

/// Kite exchange segment, from the low byte of the instrument token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Nse = 1,
    Nfo = 2,
    Cds = 3,
    Bse = 4,
    Bfo = 5,
    Bcd = 6,
    Mcx = 7,
    Mcxsx = 8,
    Indices = 9,
}

impl Segment {
    pub fn of_token(token: u32) -> Option<Segment> {
        Some(match token & 0xFF {
            1 => Segment::Nse,
            2 => Segment::Nfo,
            3 => Segment::Cds,
            4 => Segment::Bse,
            5 => Segment::Bfo,
            6 => Segment::Bcd,
            7 => Segment::Mcx,
            8 => Segment::Mcxsx,
            9 => Segment::Indices,
            _ => return None,
        })
    }

    /// Whether prices on the wire are in paise for this segment. Currency
    /// segments use other divisors and are not supported by this engine.
    pub fn prices_in_paise(self) -> bool {
        !matches!(self, Segment::Cds | Segment::Bcd)
    }
}

pub const LEN_LTP: usize = 8;
pub const LEN_INDEX_QUOTE: usize = 28;
pub const LEN_INDEX_FULL: usize = 32;
pub const LEN_QUOTE: usize = 44;
pub const LEN_FULL: usize = 184;

#[cfg(test)]
mod tests;
