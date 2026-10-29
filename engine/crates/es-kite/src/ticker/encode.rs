//! Builds Kite binary frames. Used by the simulator (so simulated ticks go
//! through the real parser) and by tests as golden fixtures.

use super::{LEN_FULL, LEN_INDEX_FULL};
use es_core::{Depth5, Paise};

/// Fields of a full mode (184 byte) packet.
#[derive(Debug, Clone, Copy, Default)]
pub struct FullPacket {
    pub token: u32,
    pub ltp: Paise,
    pub ltq: u32,
    pub avg_price: Paise,
    pub volume: u32,
    pub buy_qty: u32,
    pub sell_qty: u32,
    pub open: Paise,
    pub high: Paise,
    pub low: Paise,
    pub close: Paise,
    pub last_trade_ts: i32,
    pub oi: u32,
    pub oi_high: u32,
    pub oi_low: u32,
    pub exchange_ts: i32,
    pub depth: Depth5,
}

/// Fields of an index full (32 byte) packet.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexPacket {
    pub token: u32,
    pub ltp: Paise,
    pub high: Paise,
    pub low: Paise,
    pub open: Paise,
    pub close: Paise,
    pub change: Paise,
    pub exchange_ts: i32,
}

fn p32(v: Paise) -> [u8; 4] {
    (v.0.clamp(i32::MIN as i64, i32::MAX as i64) as i32).to_be_bytes()
}

impl FullPacket {
    pub fn encode(&self) -> [u8; LEN_FULL] {
        let mut b = [0u8; LEN_FULL];
        let mut put = |at: usize, v: [u8; 4]| b[at..at + 4].copy_from_slice(&v);
        put(0, self.token.to_be_bytes());
        put(4, p32(self.ltp));
        put(8, self.ltq.to_be_bytes());
        put(12, p32(self.avg_price));
        put(16, self.volume.to_be_bytes());
        put(20, self.buy_qty.to_be_bytes());
        put(24, self.sell_qty.to_be_bytes());
        put(28, p32(self.open));
        put(32, p32(self.high));
        put(36, p32(self.low));
        put(40, p32(self.close));
        put(44, self.last_trade_ts.to_be_bytes());
        put(48, self.oi.to_be_bytes());
        put(52, self.oi_high.to_be_bytes());
        put(56, self.oi_low.to_be_bytes());
        put(60, self.exchange_ts.to_be_bytes());
        let levels = self.depth.bids.iter().chain(self.depth.asks.iter());
        for (i, l) in levels.enumerate() {
            let o = 64 + i * 12;
            b[o..o + 4].copy_from_slice(&l.qty.to_be_bytes());
            b[o + 4..o + 8].copy_from_slice(&p32(l.price));
            b[o + 8..o + 10].copy_from_slice(&l.orders.to_be_bytes());
        }
        b
    }
}

impl IndexPacket {
    pub fn encode(&self) -> [u8; LEN_INDEX_FULL] {
        let mut b = [0u8; LEN_INDEX_FULL];
        let mut put = |at: usize, v: [u8; 4]| b[at..at + 4].copy_from_slice(&v);
        put(0, self.token.to_be_bytes());
        put(4, p32(self.ltp));
        put(8, p32(self.high));
        put(12, p32(self.low));
        put(16, p32(self.open));
        put(20, p32(self.close));
        put(24, p32(self.change));
        put(28, self.exchange_ts.to_be_bytes());
        b
    }
}

/// Collects packets into one frame.
#[derive(Debug, Default)]
pub struct FrameBuilder {
    count: u16,
    body: Vec<u8>,
}

impl FrameBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a raw packet (any mode).
    pub fn push(&mut self, packet: &[u8]) -> &mut Self {
        self.count += 1;
        self.body
            .extend_from_slice(&(packet.len() as u16).to_be_bytes());
        self.body.extend_from_slice(packet);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn finish(self) -> Vec<u8> {
        let mut f = Vec::with_capacity(2 + self.body.len());
        f.extend_from_slice(&self.count.to_be_bytes());
        f.extend_from_slice(&self.body);
        f
    }
}
