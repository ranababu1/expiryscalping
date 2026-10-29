//! Kite Connect v3 wire formats.
//!
//! - [`ticker`]: the WebSocket binary tick format (parser and encoder).
//! - [`instruments`]: the `/instruments` CSV master.
//! - [`text`]: JSON text frames (order postbacks, errors, messages).
//!
//! Reference: <https://kite.trade/docs/connect/v3/websocket/>.
//! The live connection itself arrives in Phase 2. In Phase 1 the simulator
//! and replay produce real Kite frames, so this parser runs on every tick.

pub mod instruments;
pub mod text;
pub mod ticker;
