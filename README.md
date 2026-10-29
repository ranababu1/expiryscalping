# ExpiryScalping

An automated options scalping system for NIFTY and SENSEX weekly expiry days on Zerodha Kite. A
Rust engine reads ticks, runs strategies, checks every order against a risk engine and sends orders
to a broker. A small desktop app (Tauri 2 and Svelte) shows what the engine is doing and lets you
step in: pause the AI, place, modify, cancel or exit orders, change risk limits and hit the kill
switch.

**Phase 1 (this version) is paper trading only.** There is no Kite connection, no live trading, no
VPS and no AI key. The engine trades a simulated expiry day (or a recorded tick log) with a paper
broker that fills against simulated market depth and charges realistic fees.

Full design: [docs/plan.md](docs/plan.md). Status: [docs/currentstate.md](docs/currentstate.md).
Changes: [docs/changelog.md](docs/changelog.md).

## Layout

| Folder | What |
|---|---|
| `engine/` | Rust workspace. `esd` is the engine daemon; `crates/` hold the parser, market state, strategies, risk, order manager, paper broker and the client API |
| `app/` | Desktop app: Svelte 5 UI in `src/`, Tauri 2 shell in `src-tauri/` |
| `research/` | Offline Python research (scaffold only in Phase 1) |
| `deploy/` | VPS setup (Phase 2, placeholder) |
| `docs/` | Plan, working rules, current state, changelog |

## Run it on Windows (paper mode)

### One-time setup

1. Install Git, then Rust with `rustup` from https://rustup.rs. Accept the prompt to install the
   Visual Studio C++ Build Tools ("Desktop development with C++"). Rust 1.85 or newer is needed.
2. Install Node.js 22 LTS from https://nodejs.org (20.19 or newer works).
3. WebView2 is already part of Windows 10 and 11. If the app window stays blank, install the
   "Evergreen Bootstrapper" from Microsoft's WebView2 page.
4. Optional, for research: `uv` from https://docs.astral.sh/uv/.

### Start the engine

Open PowerShell in the repo folder:

```powershell
cd engine
cargo run --release -p esd -- sim
```

The first run creates `engine\data\` and prints a **device token for the app once**. Copy it. If
you lose it, make a new one with `cargo run --release -p esd -- device add laptop` (and remove old
ones with `device list` and `device revoke <name>`).

`esd sim` trades a simulated expiry day from 09:15 to 15:30 at 10x speed and serves the app on
`127.0.0.1:7400`. Useful options:

```powershell
cargo run --release -p esd -- sim --speed 1                    # real time
cargo run --release -p esd -- sim --date 2026-11-04            # another simulated day
cargo run --release -p esd -- sim --record data\raw\day.eslog  # also record the raw ticks
cargo run --release -p esd -- replay data\raw\day.eslog        # replay a recording
cargo run --release -p esd -- sim --speed 0 --headless         # whole day, print a summary
```

Stop the engine with Ctrl+C. It writes a journal of every order, fill and risk decision to
`engine\data\journal\` and an audit log of your changes to `engine\data\audit.jsonl`.

### Start the desktop app

In a second PowerShell window:

```powershell
cd app
npm ci
npm run tauri dev
```

Paste the device token and press Connect. With "Remember" ticked, the token is kept in Windows
Credential Manager (entry `expiryscalping`), not in a file.

To build an installer instead: `npm run tauri build`. The setup file lands in
`app\src-tauri\target\release\bundle\nsis\`.

### What you can do in the app

- **Status:** mode, tick source, engine time, feed health, tick latency, P&L, kill switch state.
- **Chain:** option chain around ATM with LTP, bid, ask, IV and OI. Click an LTP to open a buy
  ticket (lots and an optional limit price).
- **Positions:** open and closed positions with P&L and charges, Exit per row, Exit all.
- **Orders:** working orders (Modify, Cancel) and today's order log.
- **Strategies:** enable or disable each strategy, Pause AI (stops new entries, exits keep running).
- **Risk:** live usage against limits, edit limits, re-arm the kill switch, recent rejections.
- **KILL** (top right): hold for about one second. It disables strategies, cancels working entries
  and flattens every position. New entries stay blocked until you re-arm on the Risk screen with a
  flat book. A kill caused by the daily max loss cannot be re-armed the same day.

Every manual order goes through the same risk checks as strategy orders. The app holds no trading
logic; it only sends commands and shows what the engine reports.

## Configuration

- `engine\config\engine.toml`: mode (paper only), data folder, API address, timing, market hours,
  underlyings, paper fill model, charges rates, strategy parameters and simulator settings.
- `engine\config\risk.toml`: risk limits (daily max loss, per trade premium and lots, open
  positions, trades per day, order rates, entry window, flatten time, stale data, spread, loss
  streak pause, margin check). Money is in paise (100 paise = 1 rupee).
- Limits changed in the app are saved to `engine\data\risk_limits.json` and win over `risk.toml`.
  Delete that file to go back to `risk.toml`. Loosening a limit during market hours needs a flat
  book and an extra confirmation.

The defaults are starting values marked for you to review. Check the charges rates against
Zerodha's current charges before trusting paper P&L.

## Development

Commands for every part are in [docs/instructions.md](docs/instructions.md) section 5. In short:

```powershell
cd engine; cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
cd app; npm run check; npm test; npm run build
cd research; uv sync; uv run ruff check .; uv run pytest
```

The TypeScript API types in `app/src/lib/api/types/` are generated from Rust by
`cargo test -p es-api`. Do not edit them by hand.

## Risk disclaimer

This is personal software for research and paper trading. It is not investment advice and comes
with no warranty. Options trading, and scalping on expiry day in particular, can lose money very
fast, including more than you expect from a stop. Paper results ignore real queue position, latency
spikes, broker outages, rejections and slippage, so live results will be worse. Simulated ticks are
not real market data. Do not trade real money with this software until the later phases are built,
tested on recorded and live data, and you have reviewed and accepted the risk limits yourself. You
are responsible for every order it places and for complying with SEBI and exchange rules.
