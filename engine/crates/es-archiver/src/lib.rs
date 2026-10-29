//! Raw tick log: an append-only file of the original frames plus our
//! receive timestamp. Replay reads it back through the same parser as live.
//!
//! Format (little-endian):
//!
//! ```text
//! magic   "ESRAW" 0x01            (6 bytes, once)
//! record  kind u8 | recv_ns i64 | len u32 | bytes[len]   (repeated)
//! ```
//!
//! `kind` is [`es_feed::RecordKind`]. A recording normally starts with one
//! `Instruments` record (the instrument master CSV) so it can be replayed on
//! its own.

use es_core::Ts;
use es_feed::{RawRecord, RecordKind};
use std::io::{self, BufReader, BufWriter, Read, Write};

pub const MAGIC: &[u8; 6] = b"ESRAW\x01";
/// Upper bound on one record, to reject corrupt lengths before allocating.
pub const MAX_RECORD: u32 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RawLogError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("not a raw tick log (bad magic)")]
    BadMagic,
    #[error("unknown record kind {0}")]
    BadKind(u8),
    #[error("record too large: {0} bytes")]
    TooLarge(u32),
}

pub struct RawLogWriter<W: Write> {
    out: BufWriter<W>,
}

impl<W: Write> RawLogWriter<W> {
    pub fn new(inner: W) -> Result<Self, RawLogError> {
        let mut out = BufWriter::new(inner);
        out.write_all(MAGIC)?;
        Ok(Self { out })
    }

    pub fn write(&mut self, r: &RawRecord) -> Result<(), RawLogError> {
        let len = u32::try_from(r.bytes.len()).map_err(|_| RawLogError::TooLarge(u32::MAX))?;
        if len > MAX_RECORD {
            return Err(RawLogError::TooLarge(len));
        }
        self.out.write_all(&[r.kind as u8])?;
        self.out.write_all(&r.recv.0.to_le_bytes())?;
        self.out.write_all(&len.to_le_bytes())?;
        self.out.write_all(&r.bytes)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), RawLogError> {
        self.out.flush()?;
        Ok(())
    }
}

pub struct RawLogReader<R: Read> {
    input: BufReader<R>,
}

impl<R: Read> RawLogReader<R> {
    pub fn new(inner: R) -> Result<Self, RawLogError> {
        let mut input = BufReader::new(inner);
        let mut magic = [0u8; 6];
        input
            .read_exact(&mut magic)
            .map_err(|_| RawLogError::BadMagic)?;
        if &magic != MAGIC {
            return Err(RawLogError::BadMagic);
        }
        Ok(Self { input })
    }

    /// Reads the next record. `Ok(None)` at a clean end of file.
    pub fn next_record(&mut self) -> Result<Option<RawRecord>, RawLogError> {
        let mut kind = [0u8; 1];
        if self.input.read(&mut kind)? == 0 {
            return Ok(None);
        }
        let kind = RecordKind::from_u8(kind[0]).ok_or(RawLogError::BadKind(kind[0]))?;
        let mut ts = [0u8; 8];
        self.input.read_exact(&mut ts)?;
        let mut len = [0u8; 4];
        self.input.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len);
        if len > MAX_RECORD {
            return Err(RawLogError::TooLarge(len));
        }
        let mut bytes = vec![0u8; len as usize];
        self.input.read_exact(&mut bytes)?;
        Ok(Some(RawRecord {
            kind,
            recv: Ts(i64::from_le_bytes(ts)),
            bytes,
        }))
    }
}

impl<R: Read> Iterator for RawLogReader<R> {
    type Item = Result<RawRecord, RawLogError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_record().transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: RecordKind, ns: i64, b: &[u8]) -> RawRecord {
        RawRecord {
            kind,
            recv: Ts(ns),
            bytes: b.to_vec(),
        }
    }

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        let records = vec![
            rec(RecordKind::Instruments, 1, b"csv"),
            rec(RecordKind::Binary, 2, &[0]),
            rec(RecordKind::Text, 3, br#"{"type":"message"}"#),
        ];
        {
            let mut w = RawLogWriter::new(&mut buf).unwrap();
            for r in &records {
                w.write(r).unwrap();
            }
            w.flush().unwrap();
        }
        let read: Vec<_> = RawLogReader::new(&buf[..])
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(read, records);
    }

    #[test]
    fn rejects_bad_magic_and_corrupt_records() {
        assert!(matches!(
            RawLogReader::new(&b"NOPE!!"[..]),
            Err(RawLogError::BadMagic)
        ));
        let mut buf = MAGIC.to_vec();
        buf.push(9);
        let mut r = RawLogReader::new(&buf[..]).unwrap();
        assert!(matches!(r.next_record(), Err(RawLogError::BadKind(9))));

        let mut buf = MAGIC.to_vec();
        buf.push(0);
        buf.extend_from_slice(&0i64.to_le_bytes());
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut r = RawLogReader::new(&buf[..]).unwrap();
        assert!(matches!(r.next_record(), Err(RawLogError::TooLarge(_))));
    }

    #[test]
    fn truncated_record_is_error() {
        let mut buf = Vec::new();
        {
            let mut w = RawLogWriter::new(&mut buf).unwrap();
            w.write(&rec(RecordKind::Binary, 5, &[1, 2, 3, 4])).unwrap();
            w.flush().unwrap();
        }
        buf.truncate(buf.len() - 2);
        let mut r = RawLogReader::new(&buf[..]).unwrap();
        assert!(r.next_record().is_err());
    }
}
