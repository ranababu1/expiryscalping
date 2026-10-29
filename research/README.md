# research

Offline research for ExpiryScalping (plan section 14). Phase 1 has only a scaffold: a reader for the
engine's raw tick logs and its test. Features, labelling, training and backtests come in Phase 4.

Rules: research never holds Kite order credentials and has no order code. It reads archived data
and produces reports, model files and setting proposals only.

```
uv sync
uv run ruff check .
uv run pytest
uv run python -m es_research.rawlog ../engine/data/raw/<file>.eslog
```
