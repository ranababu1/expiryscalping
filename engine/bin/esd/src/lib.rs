//! ExpiryScalping engine daemon (library half, so tests can drive it).
//!
//! Phase 1: paper trading only, on simulated or replayed ticks.

pub mod commands;
pub mod config;
pub mod dto;
pub mod engine;
pub mod journal;

pub use config::{EngineConfig, load_risk};
pub use engine::{Engine, EngineOptions};
pub use journal::Journal;

use es_api::ApiCommand;
use es_archiver::RawLogWriter;
use es_core::{Date, Ts};
use es_feed::RawRecord;
use std::future::Future;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Today's date in IST from the wall clock (the simulator's default day).
pub fn today_ist() -> Date {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    Ts(ns).ist_date()
}

pub fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Writes records to a raw log on its own thread so disk never blocks the
/// engine loop.
pub type Recorder = (
    std::sync::mpsc::Sender<RawRecord>,
    std::thread::JoinHandle<()>,
);

/// Returns the sender and the writer thread; drop the sender, then join the
/// thread so the log is complete before the process exits.
pub fn spawn_recorder(path: &Path) -> std::io::Result<Recorder> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path)?;
    let mut w = RawLogWriter::new(std::io::BufWriter::new(file)).map_err(std::io::Error::other)?;
    let (tx, rx) = std::sync::mpsc::channel::<RawRecord>();
    let shown = path.display().to_string();
    let handle = std::thread::spawn(move || {
        let mut n: u64 = 0;
        for r in rx {
            if let Err(e) = w.write(&r) {
                warn!(%e, "recorder: write failed, stopping");
                return;
            }
            n += 1;
        }
        if let Err(e) = w.flush() {
            warn!(%e, "recorder: flush failed");
        }
        info!(records = n, file = %shown, "recorder: done");
    });
    Ok((tx, handle))
}

pub struct RunOptions {
    pub publish_every: Duration,
    pub recorder: Option<std::sync::mpsc::Sender<RawRecord>>,
    /// Stop when the tick source ends (headless runs). Otherwise keep serving
    /// the app with frozen prices.
    pub exit_when_done: bool,
}

/// The engine loop: records, client commands and snapshot publishing.
pub async fn run(
    mut engine: Engine,
    mut records: mpsc::Receiver<RawRecord>,
    mut commands: mpsc::Receiver<ApiCommand>,
    opts: RunOptions,
    stop: impl Future<Output = ()>,
) -> Engine {
    tokio::pin!(stop);
    let mut publish = tokio::time::interval(opts.publish_every);
    publish.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut source_open = true;
    loop {
        tokio::select! {
            biased;
            _ = &mut stop => {
                info!("stopping");
                break;
            }
            Some(c) = commands.recv() => engine.on_command(c).await,
            r = records.recv(), if source_open => match r {
                Some(r) => {
                    if let Some(tx) = &opts.recorder {
                        let _ = tx.send(r.clone());
                    }
                    engine.on_record(r).await;
                }
                None => {
                    source_open = false;
                    engine.mark_replay_done();
                    engine.publish_all();
                    if opts.exit_when_done {
                        break;
                    }
                }
            },
            _ = publish.tick() => engine.publish_all(),
        }
    }
    engine.shutdown();
    engine
}
