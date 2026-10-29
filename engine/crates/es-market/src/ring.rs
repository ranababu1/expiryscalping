//! Fixed-capacity ring of (time, price) samples, at most one per second.

use es_core::{Paise, Ts};

pub(crate) struct Ring {
    buf: Vec<(Ts, Paise)>,
    head: usize,
    len: usize,
}

impl Ring {
    pub(crate) fn new(cap: usize) -> Ring {
        Ring {
            buf: vec![(Ts(0), Paise::ZERO); cap.max(1)],
            head: 0,
            len: 0,
        }
    }

    fn idx(&self, i: usize) -> usize {
        (self.head + self.buf.len() - self.len + i) % self.buf.len()
    }

    pub(crate) fn push(&mut self, ts: Ts, p: Paise) {
        if let Some((last_ts, _)) = self.latest()
            && ts.secs() == last_ts.secs()
        {
            // Same second: keep the latest price.
            let i = self.idx(self.len - 1);
            self.buf[i] = (ts, p);
            return;
        }
        self.buf[self.head] = (ts, p);
        self.head = (self.head + 1) % self.buf.len();
        self.len = (self.len + 1).min(self.buf.len());
    }

    pub(crate) fn latest(&self) -> Option<(Ts, Paise)> {
        (self.len > 0).then(|| self.buf[self.idx(self.len - 1)])
    }

    /// Oldest sample at or after `from`.
    pub(crate) fn at_or_after(&self, from: Ts) -> Option<(Ts, Paise)> {
        (0..self.len)
            .map(|i| self.buf[self.idx(i)])
            .find(|(t, _)| *t >= from)
    }
}
