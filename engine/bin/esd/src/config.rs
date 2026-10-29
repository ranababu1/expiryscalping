//! Engine configuration. Files live in `engine/config/`:
//!
//! * `engine.toml`: mode, paths, timings, paper broker, charges, strategies, simulator.
//! * `risk.toml`: risk limits. Overrides saved from the app go to
//!   `<data_dir>/risk_limits.json`, which wins when present.

use es_broker::{ChargesConfig, PaperConfig};
use es_market::UnderlyingSpec;
use es_oms::KiteLimits;
use es_replay::SimConfig;
use es_risk::{MarketHours, RiskLimits};
use es_strategy::MomentumConfig;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("bad config in {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigMode {
    Paper,
    Live,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// Engine timer step, in engine (tick) time.
    pub timer_ms: i64,
    /// How often snapshots go to clients, in wall-clock time.
    pub publish_ms: u64,
    /// Unfilled entry orders are cancelled after this.
    pub entry_timeout_ms: i64,
    /// A working exit order is cancelled and re-priced after this.
    pub exit_chase_ms: i64,
    /// If an exit is still not done after this, it goes out as a market order
    /// with protection.
    pub market_exit_after_ms: i64,
    /// An order without a broker answer after this becomes Unknown and is
    /// reconciled.
    pub sent_timeout_ms: i64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            timer_ms: 250,
            publish_ms: 200,
            entry_timeout_ms: 3_000,
            exit_chase_ms: 2_000,
            market_exit_after_ms: 6_000,
            sent_timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketConfig {
    pub hours: MarketHours,
    pub underlyings: Vec<UnderlyingSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MomentumSlot {
    pub enabled: bool,
    pub params: MomentumConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategiesConfig {
    pub momentum: MomentumSlot,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    pub mode: ConfigMode,
    /// Relative paths resolve against the config file's folder.
    pub data_dir: PathBuf,
    pub api_bind: String,
    pub timing: Timing,
    pub market: MarketConfig,
    pub paper: PaperConfig,
    pub charges: ChargesConfig,
    pub kite_limits: KiteLimits,
    pub strategies: StrategiesConfig,
    pub sim: SimConfig,
}

fn read(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path, text: &str) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

impl EngineConfig {
    pub fn load(path: &Path) -> Result<EngineConfig, ConfigError> {
        let mut cfg: EngineConfig = parse(path, &read(path)?)?;
        if cfg.data_dir.is_relative()
            && let Some(dir) = path.parent()
        {
            cfg.data_dir = dir.join(&cfg.data_dir);
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn from_toml(text: &str) -> Result<EngineConfig, ConfigError> {
        let cfg: EngineConfig = parse(Path::new("<inline>"), text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let t = &self.timing;
        if t.timer_ms <= 0 || t.publish_ms == 0 {
            return Err(ConfigError::Invalid(
                "timing.timer_ms and timing.publish_ms must be positive".into(),
            ));
        }
        if t.market_exit_after_ms < t.exit_chase_ms {
            return Err(ConfigError::Invalid(
                "timing.market_exit_after_ms must be at least exit_chase_ms".into(),
            ));
        }
        if self.market.underlyings.is_empty() {
            return Err(ConfigError::Invalid("market.underlyings is empty".into()));
        }
        Ok(())
    }
}

/// `risk.toml` has a single `[limits]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RiskFile {
    limits: RiskLimits,
}

/// Loads risk limits: the saved override if present, else `risk.toml`.
/// Returns the limits and where they came from.
pub fn load_risk(risk_toml: &Path, data_dir: &Path) -> Result<(RiskLimits, PathBuf), ConfigError> {
    let saved = data_dir.join(RISK_OVERRIDE_FILE);
    let (limits, from) = if saved.exists() {
        let text = read(&saved)?;
        let l: RiskLimits = serde_json::from_str(&text).map_err(|e| ConfigError::Parse {
            path: saved.clone(),
            message: e.to_string(),
        })?;
        (l, saved)
    } else {
        let f: RiskFile = parse(risk_toml, &read(risk_toml)?)?;
        (f.limits, risk_toml.to_path_buf())
    };
    limits
        .validate()
        .map_err(|e| ConfigError::Invalid(format!("risk limits in {}: {e}", from.display())))?;
    Ok((limits, from))
}

pub const RISK_OVERRIDE_FILE: &str = "risk_limits.json";

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config")
    }

    #[test]
    fn shipped_engine_config_parses() {
        let cfg = EngineConfig::load(&root().join("engine.toml")).unwrap();
        assert_eq!(cfg.mode, ConfigMode::Paper);
        assert!(cfg.strategies.momentum.enabled);
    }

    #[test]
    fn shipped_risk_config_matches_plan_defaults() {
        let dir = std::env::temp_dir().join("esd-no-override");
        let (l, _) = load_risk(&root().join("risk.toml"), &dir).unwrap();
        assert_eq!(l, RiskLimits::default());
    }

    #[test]
    fn override_wins() {
        let dir = std::env::temp_dir().join(format!("esd-override-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let l = RiskLimits {
            per_trade_max_lots: 3,
            ..RiskLimits::default()
        };
        std::fs::write(
            dir.join(RISK_OVERRIDE_FILE),
            serde_json::to_string(&l).unwrap(),
        )
        .unwrap();
        let (got, from) = load_risk(&root().join("risk.toml"), &dir).unwrap();
        assert_eq!(got.per_trade_max_lots, 3);
        assert!(from.ends_with(RISK_OVERRIDE_FILE));
        let _ = std::fs::remove_dir_all(dir);
    }
}
