//! Device tokens. Each client device has a long random token. The engine
//! stores only a SHA-256 hash, compares in constant time, and knows each
//! device by name for the audit log. Tokens are created and revoked on the
//! engine machine (`esd device add|list|revoke`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("device store io: {0}")]
    Io(#[from] std::io::Error),
    #[error("device store is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("a device named {0:?} already exists")]
    Exists(String),
    #[error("no device named {0:?}")]
    NotFound(String),
    #[error("device names are 1 to 32 letters, digits, '-' or '_'")]
    BadName,
    #[error("no secure random source: {0}")]
    Random(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    /// Hex SHA-256 of the token.
    pub token_sha256: String,
    pub created_ms: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeviceStore {
    pub devices: Vec<Device>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

fn hash(token: &str) -> [u8; 32] {
    let d = Sha256::digest(token.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&d);
    out
}

/// Constant-time equality for fixed-size digests.
fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl DeviceStore {
    /// Loads the store, or an empty one if the file does not exist yet.
    pub fn load(path: &Path) -> Result<DeviceStore, DeviceError> {
        let mut s = match std::fs::read_to_string(path) {
            Ok(txt) => serde_json::from_str(&txt)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DeviceStore::default(),
            Err(e) => return Err(e.into()),
        };
        s.path = Some(path.to_path_buf());
        Ok(s)
    }

    pub fn save(&self) -> Result<(), DeviceError> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, p)?;
        Ok(())
    }

    /// Adds a device and returns its token. The token is shown once and
    /// never stored.
    pub fn add(&mut self, name: &str, now_ms: i64) -> Result<String, DeviceError> {
        if !valid_name(name) {
            return Err(DeviceError::BadName);
        }
        if self.devices.iter().any(|d| d.name == name) {
            return Err(DeviceError::Exists(name.into()));
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| DeviceError::Random(e.to_string()))?;
        let token = format!("es_{}", hex::encode(bytes));
        self.devices.push(Device {
            name: name.into(),
            token_sha256: hex::encode(hash(&token)),
            created_ms: now_ms,
        });
        Ok(token)
    }

    pub fn revoke(&mut self, name: &str) -> Result<(), DeviceError> {
        let before = self.devices.len();
        self.devices.retain(|d| d.name != name);
        if self.devices.len() == before {
            return Err(DeviceError::NotFound(name.into()));
        }
        Ok(())
    }

    /// Returns the device name for a valid token. Every stored hash is
    /// compared, in constant time, whatever matches.
    pub fn verify(&self, token: &str) -> Option<String> {
        let h = hash(token);
        let mut found = None;
        for d in &self.devices {
            let mut stored = [0u8; 32];
            if hex::decode_to_slice(&d.token_sha256, &mut stored).is_err() {
                continue;
            }
            if ct_eq(&h, &stored) && found.is_none() {
                found = Some(d.name.clone());
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_verify_revoke() {
        let mut s = DeviceStore::default();
        let t = s.add("desktop", 1).unwrap();
        assert!(t.starts_with("es_") && t.len() == 67);
        assert_eq!(s.verify(&t).as_deref(), Some("desktop"));
        assert_eq!(s.verify("es_wrong"), None);
        assert_eq!(s.verify(""), None);
        let mut almost = t.clone();
        almost.pop();
        almost.push('x');
        assert_eq!(s.verify(&almost), None);
        assert!(matches!(s.add("desktop", 2), Err(DeviceError::Exists(_))));
        assert!(matches!(s.add("bad name", 2), Err(DeviceError::BadName)));
        s.revoke("desktop").unwrap();
        assert_eq!(s.verify(&t), None);
        assert!(matches!(s.revoke("desktop"), Err(DeviceError::NotFound(_))));
    }

    #[test]
    fn store_never_contains_the_token() {
        let mut s = DeviceStore::default();
        let t = s.add("phone", 1).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains(&t[3..]));
    }

    #[test]
    fn load_missing_is_empty_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("es-api-test-{}", std::process::id()));
        let p = dir.join("devices.json");
        let mut s = DeviceStore::load(&p).unwrap();
        assert!(s.devices.is_empty());
        let t = s.add("desktop", 5).unwrap();
        s.save().unwrap();
        let s2 = DeviceStore::load(&p).unwrap();
        assert_eq!(s2.verify(&t).as_deref(), Some("desktop"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
