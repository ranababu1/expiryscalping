# Changelog

Newest first. Every release gets an entry. Format is in docs/instructions.md.

## 2026-10-30 (release: v0.3.0 - Phase 1 complete: desktop app on paper)

### Added
- Desktop app (app/): Svelte 5, TypeScript strict, Vite, using the API types generated from Rust. Dark, minimal layout with a top bar (mode, source, engine time IST, feed health, p99 latency, total P&L, Pause AI, KILL) and six screens: Status, Chain, Positions, Orders, Strategies, Risk.
- Manual intervention: buy ticket from any chain LTP (lots and an optional limit), Modify and Cancel on working orders, Exit per position, Exit all with a confirm, Pause and Resume AI, strategy on and off, risk limits editor with the loosening confirmation, re-arm.
- Hold-to-confirm kill switch (about 1.2 s, mouse or keyboard), with component tests.
- Engine client with auth first, request ids, replies, timeouts and reconnect with backoff. Loading, empty, error (toasts) and disconnected (banner, dimmed data) states.
- Connect screen. The device token is stored in Windows Credential Manager through the Tauri shell (keyring), or kept in memory only in a plain browser.
- Tauri 2 shell (app/src-tauri) with token commands, icons and NSIS and MSI bundle targets.
- research/ scaffold: uv project, ruff, raw tick log reader with tests.
- deploy/ placeholder README, root README (Windows paper mode steps, config, risk disclaimer).
- GitHub Actions CI: engine fmt, clippy, tests and a stale generated types check; app check, tests and build; desktop shell clippy on Windows; research ruff and pytest.

### Changed
- StatusDto field docs for `source` and `kite` match what the engine sends.
- .gitignore covers `.eslog` recordings and the Tauri gen/ folder.

### Fixed
- p50 latency under one microsecond shows as "<1 µs" instead of 0.

### Verification
- cargo test --workspace: passed (162 tests)
- cargo clippy --workspace --all-targets -- -D warnings: clean; cargo fmt --check: clean
- app: npm run check (0 errors, 0 warnings), npm test (23 tests), npm run build: passed
- app/src-tauri: cargo clippy --all-targets -- -D warnings clean; `tauri build --bundles deb` built on Linux; CI clippy on windows-latest green
- research: ruff check, ruff format --check, pytest (3 tests): passed
- Browser smoke test against `esd sim`: connect, chain, place a manual buy, orders and positions update, short press does not kill, hold kills and flattens, re-arm, edit limits, pause AI, disconnect banner when the engine stops.

### Git
- Commits: `fe38e42` to `1f2e9a7`, plus this release commit
- Pushed to GitHub: yes

## 2026-10-30 (release: v0.2.0 - client API served by the engine)

### Added
- esd serves the client WebSocket API on 127.0.0.1:7400 while it paper trades: status, chain, positions, orders, fills, risk, strategies and alerts streams, plus every command the app needs (kill, re-arm, pause, strategy on and off, risk limits, place, modify, cancel, exit, exit all, chain selection).
- First run creates a "desktop" device token and prints it once. `esd device add|list|revoke` manages more.
- End-to-end test over a real WebSocket (bin/esd/tests/api.rs): bad token refused, hello and snapshots arrive, the kill switch acks, alerts and shows in status.

### Changed
- The Strategy trait requires Send + Sync so the engine loop can run as a spawned task.

### Verification
- cargo test --workspace: passed (162 tests)
- cargo clippy --workspace --all-targets -- -D warnings: clean

### Git
- Commits: `6b8094c`, plus this release commit
- Pushed to GitHub: yes

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
