//! Kite's own order limits, enforced on our side so we never hit them.

use es_core::Ts;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KiteLimits {
    /// Kite allows 10 per second; we stay below it.
    pub per_sec: u32,
    /// Kite allows 400 per minute; we stay below it.
    pub per_min: u32,
    /// Kite allows 5,000 per day; we stay below it.
    pub per_day: u32,
    /// Kite allows 25 modifications per order, then cancel and re-place.
    pub max_modifications: u32,
}

impl Default for KiteLimits {
    fn default() -> Self {
        KiteLimits {
            per_sec: 9,
            per_min: 390,
            per_day: 4_900,
            max_modifications: 25,
        }
    }
}

/// Counts order API calls (place, modify, cancel) in sliding windows.
#[derive(Debug, Default)]
pub struct CallCounter {
    recent: VecDeque<Ts>,
    day: Option<i64>,
    today: u32,
}

impl CallCounter {
    fn roll(&mut self, now: Ts) {
        let d = now.ist_date().days();
        if self.day != Some(d) {
            self.day = Some(d);
            self.today = 0;
        }
        while let Some(&f) = self.recent.front() {
            if now.ms_since(f) >= 60_000 {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }

    /// True if one more call fits all windows.
    pub fn allows(&mut self, l: &KiteLimits, now: Ts) -> bool {
        self.roll(now);
        let last_sec = self
            .recent
            .iter()
            .rev()
            .take_while(|t| now.ms_since(**t) < 1_000)
            .count() as u32;
        last_sec < l.per_sec && (self.recent.len() as u32) < l.per_min && self.today < l.per_day
    }

    pub fn record(&mut self, now: Ts) {
        self.roll(now);
        self.recent.push_back(now);
        self.today += 1;
    }

    pub fn today(&self) -> u32 {
        self.today
    }
}
