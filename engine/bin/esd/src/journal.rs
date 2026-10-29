//! Append-only journal of signals, risk decisions, orders and fills, one JSON
//! object per line. It is the audit trail for a session and what the replay
//! determinism test compares.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use tracing::warn;

#[derive(Default)]
pub struct Journal {
    file: Option<BufWriter<File>>,
    /// Kept in memory when enabled (tests).
    mem: Option<Vec<String>>,
}

impl Journal {
    pub fn to_file(path: &Path) -> std::io::Result<Journal> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let f = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Journal {
            file: Some(BufWriter::new(f)),
            mem: None,
        })
    }

    pub fn in_memory() -> Journal {
        Journal {
            file: None,
            mem: Some(Vec::new()),
        }
    }

    pub fn write<T: Serialize>(&mut self, kind: &str, ts: i64, body: &T) {
        if self.file.is_none() && self.mem.is_none() {
            return;
        }
        let line = match serde_json::to_string(&serde_json::json!({
            "kind": kind,
            "ts": ts,
            "data": body,
        })) {
            Ok(l) => l,
            Err(e) => {
                warn!(%e, "journal: cannot serialise entry");
                return;
            }
        };
        if let Some(f) = self.file.as_mut()
            && let Err(e) = writeln!(f, "{line}")
        {
            warn!(%e, "journal: write failed");
        }
        if let Some(m) = self.mem.as_mut() {
            m.push(line);
        }
    }

    pub fn flush(&mut self) {
        if let Some(f) = self.file.as_mut()
            && let Err(e) = f.flush()
        {
            warn!(%e, "journal: flush failed");
        }
    }

    pub fn lines(&self) -> &[String] {
        self.mem.as_deref().unwrap_or(&[])
    }
}
