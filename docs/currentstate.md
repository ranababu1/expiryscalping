# Current state

**Phase:** 1 (engine core, paper trading, desktop app on simulated and replayed ticks)

## What works
- Engine (v0.2.0): Kite tick parser, raw log, simulator and replay, market state, momentum strategy, risk engine with kill switch, order manager, paper broker with charges.
- `esd sim` and `esd replay` run a full paper trading day headless or with the client API on 127.0.0.1:7400.
- Replays are deterministic (tested). The client API is tested end to end over a WebSocket.

## In progress
- Tauri 2 and Svelte desktop app.

## Next steps
- Desktop app screens: Status, Chain, Positions, Orders, Strategies, Risk, kill switch, manual ticket.
- research/ scaffold, README, CI.

## Open decisions (picked the simplest option, change if needed)
- Entries are option buys only. Short entries are rejected (selling margin does not fit under Rs 1 lakh).
- Kill switch always needs a manual re-arm with a flat book. A daily max loss trigger cannot be re-armed the same day. Strategies stay off after a re-arm until enabled.
- Exits are exempt from our order rate cap but still counted. The order manager rate limits creates and modifies, never cancels.
- Engine time comes from tick receive time, not the wall clock, so replays are deterministic.
- The order manager does no IO; the engine calls the broker and reports results back. Broker calls are awaited inline (paper calls are instant); spawning them is a Phase 3 concern.
- Fills found during reconcile are booked at the order limit price until the trade book arrives in Phase 3.
- Manual orders are signals from strategy "manual" and pass the same risk checks. Cancelling a manual exit stops it; kill switch and flatten exits keep retrying.
- Exits are re-priced every 2 s and go out as protected market orders after 6 s.
- Charges rates are config values. Verify against Zerodha before trusting paper P&L.

## Last updated
2026-10-30

## Last commit
`chore(release): bump version to 0.2.0` (see `git log -1`).
