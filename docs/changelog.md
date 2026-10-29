# Changelog

Newest first. Every release gets an entry. Format is in docs/instructions.md.

## 2026-10-30 (unreleased - Phase 1 in progress)

### Added
- Cargo workspace under engine/ with es-core, es-kite, es-feed, es-archiver, es-replay, es-market, es-risk, es-broker.
- Kite binary tick parser with golden and fuzz tests, instruments CSV and text message parsing.
- Raw log archiver format and reader, tick simulator and replay source.
- Market state: quotes, option chain, ATM, IV.
- Risk engine with every plan limit, kill switch, order rate cap.
- Paper broker with depth fills, partials, latency, slippage and a charges model. Kite broker stub for Phase 3.

### Verification
- cargo test per crate as each was added.

### Git
- Small commits pushed to main.
