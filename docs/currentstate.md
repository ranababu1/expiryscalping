# Current state

**Phase:** 1 (engine core, paper trading, desktop app on simulated and replayed ticks)

## What works
- Engine crates: core types, Kite tick parser, raw log, simulator and replay, market state, risk engine, paper broker.

## In progress
- Order manager (es-oms), strategy (es-strategy), engine binary (esd).

## Next steps
- Client WebSocket API (es-api) with generated TypeScript types.
- Tauri 2 and Svelte desktop app.
- research/ scaffold, README.

## Open decisions (picked the simplest option, change if needed)
- Entries are option buys only. Short entries are rejected (selling margin does not fit under Rs 1 lakh).
- Kill switch always needs a manual re-arm with a flat book. A daily max loss trigger cannot be re-armed the same day.
- Exits are exempt from our order rate cap but still counted.
- Engine time comes from tick receive time, not the wall clock, so replays are deterministic.
- Charges rates are config values. Verify against Zerodha before trusting paper P&L.

## Last updated
2026-10-30

## Last commit
See `git log -1`.
