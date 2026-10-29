# Current state

**Phase:** 1 complete (v0.3.0). Waiting for Imran's review of the Phase 1 milestone before Phase 2.

## What works
- Engine: Kite tick parser, raw log, simulator and replay, market state, momentum strategy, risk engine with kill switch, order manager, paper broker with charges, client API on 127.0.0.1:7400.
- `esd sim` and `esd replay` run a full paper trading day, headless or with the app. Replays are deterministic (tested).
- Desktop app: Status, Chain, Positions, Orders, Strategies, Risk, hold-to-confirm kill switch, manual buy, modify, cancel, exit, exit all, pause AI, risk limits editor, re-arm. Token in Windows Credential Manager.
- research/ scaffold with a raw log reader. CI on GitHub Actions for engine, app, desktop shell (Windows) and research.

## In progress
- Nothing.

## Next steps
- Imran: run the Phase 1 milestone on Windows (README), review the default limits and charges.
- Phase 2 (not started): Kite login and session, live feed, archiver, VPS. Decide whether the archiver ships first.

## Open decisions (picked the simplest option, change if needed)
- Entries are option buys only. Short entries are rejected (selling margin does not fit under Rs 1 lakh).
- Kill switch always needs a manual re-arm with a flat book. A daily max loss kill cannot be re-armed the same day. Strategies stay off after a re-arm.
- Exits are exempt from our order rate cap but still counted. The order manager rate limits creates and modifies, never cancels.
- Engine time comes from tick receive time, so replays are deterministic.
- The order manager does no IO; broker calls are awaited inline (paper calls are instant).
- Reconcile fills are booked at the order limit price until the trade book arrives in Phase 3.
- Manual orders are signals from strategy "manual" through the same risk checks. Cancelling a manual exit stops it; kill and flatten exits keep retrying. Exits are re-priced every 2 s and go out as protected market orders after 6 s.
- App: manual tickets are buys only (exits go through Exit). A blank limit means a marketable limit set by the engine. Kill hold time is 1.2 s.
- App token: OS credential store in the desktop app; memory only in browser dev mode. Engine URL is saved in localStorage.
- Charges rates are config values. Verify against Zerodha before trusting paper P&L.

## Last updated
2026-10-30

## Last commit
`chore(release): bump version to 0.3.0` (see `git log -1`).
