# Changelog

Newest first. Every release gets an entry. Format is in docs/instructions.md.

## 2026-10-30 (release: v0.1.0 - engine core, headless paper trading)

### Added
- Cargo workspace under engine/: es-core, es-kite, es-feed, es-archiver, es-replay, es-market, es-strategy, es-risk, es-oms, es-broker, es-api and the esd binary.
- Kite binary tick parser with golden and fuzz tests, instruments CSV and text message parsing (es-kite).
- Raw log format, writer and reader (es-archiver); seeded tick simulator and paced replay source (es-replay).
- Market state: quotes, option chain, ATM, IV (es-market).
- Strategy trait and one documented rule strategy, momentum breakout (es-strategy, 8 tests).
- Risk engine with every plan limit, kill switch and order rate cap (es-risk, 31 tests covering each rule).
- Order manager state machine with Kite API rate limits, reconciliation and positions (es-oms, 27 tests).
- Paper broker with depth walking fills in whole lots, partials, latency, slippage and a charges model; Kite broker stub for Phase 3 (es-broker, 12 tests).
- Client WebSocket API crate with token auth and generated TypeScript types (es-api, 36 tests). Wired into esd in this release.
- esd engine daemon: `esd sim`, `esd replay`, `esd device`, `--record`, `--headless`. Config in engine/config/engine.toml and engine/config/risk.toml (defaults marked for Imran to change), overrides saved to data/risk_limits.json, audit log in data/audit.jsonl, session journal in data/journal/.
- End-to-end engine tests (bin/esd/tests/engine.rs): replay of a recorded sim day gives an identical journal, kill switch flattens and blocks entries, flatten time, daily max loss kill that cannot re-arm the same day, manual place and exit through risk, pause, live mode refused, loosening limits needs confirmation.

### Changed
- Simulator config in engine.toml uses 10 strikes each side so the chain follows the index through the day.

### Verification
- cargo test --workspace: passed (161 tests)
- cargo clippy --workspace --all-targets -- -D warnings: clean
- `esd sim --speed 0 --headless --record` then `esd replay --speed 0 --headless` on the recording: identical session summaries.

### Git
- Commits: `fe10f05` to `d287990`, plus this release commit
- Pushed to GitHub: yes
