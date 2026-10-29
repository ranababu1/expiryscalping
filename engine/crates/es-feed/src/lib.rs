//! Feed records and health tracking.
//!
//! Every tick source (live Kite in Phase 2, the simulator and replay now)
//! produces [`RawRecord`]s: the original wire bytes plus a receive time. The
//! engine parses them with the same Kite parser in every mode, so replays
//! and backtests exercise the real code path.

use es_core::Ts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// A binary WebSocket frame (ticks or heartbeat).
    Binary = 0,
    /// An instrument master CSV dump.
    Instruments = 1,
    /// A JSON text frame (order postbacks, errors, messages).
    Text = 2,
}

impl RecordKind {
    pub fn from_u8(v: u8) -> Option<RecordKind> {
        Some(match v {
            0 => RecordKind::Binary,
            1 => RecordKind::Instruments,
            2 => RecordKind::Text,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub kind: RecordKind,
    pub recv: Ts,
    pub bytes: Vec<u8>,
}

/// Tracks whether the feed is connected and when it last delivered a frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct FeedHealth {
    connected: bool,
    last_frame: Option<Ts>,
    frames: u64,
}

impl FeedHealth {
    pub fn on_connect(&mut self) {
        self.connected = true;
    }

    pub fn on_disconnect(&mut self) {
        self.connected = false;
    }

    pub fn on_frame(&mut self, at: Ts) {
        self.last_frame = Some(at);
        self.frames += 1;
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn last_frame(&self) -> Option<Ts> {
        self.last_frame
    }

    /// Stale if disconnected, never seen a frame, or silent for over `max_ms`.
    pub fn is_stale(&self, now: Ts, max_ms: i64) -> bool {
        match (self.connected, self.last_frame) {
            (true, Some(t)) => now.ms_since(t) > max_ms,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness() {
        let mut h = FeedHealth::default();
        assert!(h.is_stale(Ts(0), 1_500));
        h.on_connect();
        h.on_frame(Ts::from_millis(1_000));
        assert!(!h.is_stale(Ts::from_millis(2_500), 1_500));
        assert!(h.is_stale(Ts::from_millis(2_501), 1_500));
        h.on_disconnect();
        assert!(h.is_stale(Ts::from_millis(1_000), 1_500));
    }
}
