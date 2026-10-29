//! Tick sources that need no Kite connection.
//!
//! - [`sim::Simulator`]: a seeded synthetic expiry day (index random walk
//!   with trend bursts, an option chain priced with Black-Scholes, OI and
//!   5-level depth), emitted as real Kite binary frames.
//! - [`open_replay`]: reads a raw tick log recorded by the engine.
//! - [`pace`]: feeds any record stream to the engine at a chosen speed.

pub mod rng;
pub mod sim;

pub use sim::{SimConfig, SimUnderlying, Simulator, example_config};

use es_archiver::{RawLogError, RawLogReader};
use es_feed::RawRecord;
use std::fs::File;
use std::path::Path;
use tokio::sync::mpsc;

/// Opens a raw tick log for replay.
pub fn open_replay(path: &Path) -> Result<RawLogReader<File>, RawLogError> {
    RawLogReader::new(File::open(path)?)
}

/// Sends records to `tx`, sleeping so that recorded time passes `speed`
/// times faster than real time. `speed <= 0` means as fast as possible.
/// Returns the number of records sent. Stops early if the receiver closes.
pub async fn pace<I>(records: I, speed: f64, tx: mpsc::Sender<RawRecord>) -> u64
where
    I: Iterator<Item = RawRecord> + Send,
{
    let mut sent = 0;
    let mut prev: Option<es_core::Ts> = None;
    for r in records {
        if speed > 0.0
            && let Some(p) = prev
        {
            let gap_ns = (r.recv.0 - p.0).max(0) as f64 / speed;
            if gap_ns >= 1_000_000.0 {
                tokio::time::sleep(std::time::Duration::from_nanos(gap_ns as u64)).await;
            }
        }
        prev = Some(r.recv);
        if tx.send(r).await.is_err() {
            break;
        }
        sent += 1;
    }
    sent
}
