//! Sliding-window order rate cap. Storage is reserved up front, so recording
//! an order does not allocate.

use es_core::Ts;
use std::collections::VecDeque;

#[derive(Debug)]
pub(crate) struct RateWindow {
    sent: VecDeque<Ts>,
}

impl RateWindow {
    pub(crate) fn new(per_min: u32) -> RateWindow {
        RateWindow {
            sent: VecDeque::with_capacity(per_min as usize + 16),
        }
    }

    fn prune(&mut self, now: Ts) {
        while let Some(&t) = self.sent.front() {
            if now.ms_since(t) >= 60_000 {
                self.sent.pop_front();
            } else {
                break;
            }
        }
    }

    pub(crate) fn counts(&mut self, now: Ts) -> (u32, u32) {
        self.prune(now);
        let last_sec = self
            .sent
            .iter()
            .filter(|&&t| now.ms_since(t) < 1_000)
            .count();
        (last_sec as u32, self.sent.len() as u32)
    }

    pub(crate) fn allows(&mut self, now: Ts, per_sec: u32, per_min: u32) -> bool {
        let (s, m) = self.counts(now);
        s < per_sec && m < per_min
    }

    pub(crate) fn record(&mut self, now: Ts) {
        self.prune(now);
        self.sent.push_back(now);
    }

    pub(crate) fn reserve(&mut self, per_min: u32) {
        let want = per_min as usize + 16;
        if self.sent.capacity() < want {
            self.sent.reserve(want - self.sent.len());
        }
    }
}
