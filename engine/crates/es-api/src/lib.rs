//! Client WebSocket API. Carries messages between clients and the engine.
//! It holds no trading logic: commands are forwarded to the engine, which
//! runs them through the risk engine and order manager.

pub mod auth;
pub mod msg;
pub mod server;

pub use auth::{Device, DeviceError, DeviceStore};
pub use msg::*;
pub use server::{ApiCommand, Publisher, serve};
