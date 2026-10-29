//! Instruments. Lot size, tick size and expiry come from the instrument
//! master (or the simulator's master), never from code constants.

use crate::{Date, Paise};
use serde::{Deserialize, Serialize};

/// An index the engine trades options on, for example `NIFTY` or `SENSEX`.
/// This is the `name` column of the Kite instrument master.
pub type Underlying = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentKind {
    /// The index itself (spot). Not tradable.
    Index,
    /// Call option.
    Ce,
    /// Put option.
    Pe,
    /// Index future.
    Fut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instrument {
    pub token: u32,
    pub tradingsymbol: String,
    /// Underlying name, for example `NIFTY`.
    pub underlying: Underlying,
    pub exchange: String,
    pub kind: InstrumentKind,
    pub expiry: Option<Date>,
    /// Strike price in paise (zero for index and futures).
    pub strike: Paise,
    pub tick_size: Paise,
    /// Units per lot.
    pub lot_size: u32,
}

impl Instrument {
    pub fn is_option(&self) -> bool {
        matches!(self.kind, InstrumentKind::Ce | InstrumentKind::Pe)
    }

    pub fn expires_on(&self, date: Date) -> bool {
        self.expiry == Some(date)
    }
}
