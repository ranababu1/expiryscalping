//! Shared types used by every engine crate.
//!
//! Money is always integer paise ([`Paise`]). Time is nanoseconds since the
//! Unix epoch ([`Ts`]), so replays are deterministic and never read the wall
//! clock inside trading logic.

pub mod instrument;
pub mod money;
pub mod order;
pub mod signal;
pub mod tick;
pub mod time;

pub use instrument::{Instrument, InstrumentKind, Underlying};
pub use money::Paise;
pub use order::{Fill, OrderId, OrderType, Product, Side};
pub use signal::{Intent, ReasonCode, Signal};
pub use tick::{Depth5, DepthLevel, Tick};
pub use time::{Date, Ts};
