# ExpiryScalping - Working Instructions

These rules apply to anyone working on this repository, human or AI. Read this file, `docs/currentstate.md` and the latest entries of `docs/changelog.md` before changing anything. The product and architecture are defined in `docs/plan.md`.

---

## 1. Start of every session

The repository is the source of truth. Do not rely on conversation history.

```powershell
git status
git branch --show-current
git remote -v
git log -10 --oneline
```

Then read:

1. `docs/currentstate.md` (what is built, what is in progress, what is next)
2. The latest entries in `docs/changelog.md`
3. The relevant sections of `docs/plan.md`

Use them to understand what was recently built, what was pushed, what is incomplete and where development is heading.

---

## 2. How to work

1. Inspect the existing code before changing anything.
2. Do not overwrite existing work unnecessarily. Preserve working functionality.
3. Work in small vertical slices: one feature end to end, verified, then the next.
4. Keep the first implementation simple. Do not build future phases early.
5. Do not leave placeholder implementations where real behaviour is expected.
6. Do not add abstractions without a current need.
7. Use strongly typed models. Money is integer paise, never floating point.
8. Keep files and functions small. Avoid giant files.
9. When a feature is finished, verify the whole user flow, not just the unit.
10. If something is ambiguous, pick the simplest design that keeps future options open, and note the decision in `docs/currentstate.md`.

---

## 3. Trading safety rules (non-negotiable)

1. **Every order goes through the risk engine.** No code path may build or send an order that skipped `es-risk`. Do not weaken Rust visibility rules that enforce this.
2. **Never add a way to disable the risk engine**, by config, flag, API message or environment variable.
3. **Exits must always be allowed.** No rule may block an exit or the kill switch.
4. **Live trading decisions come from the in-engine models, within risk limits, with manual override always available.** Imran can intervene at any time to edit, modify, place or exit trades. Jev and the LLM work offline and through approved settings, since network calls are too slow for live order decisions.
5. **Paper is the default mode.** Do not change defaults that would start the engine in live mode.
6. **Any change to risk rules, the order manager, the broker or the kill switch needs tests** covering the change, including the boundary cases. No exceptions.
7. **Never double-place.** Orders in unknown state are reconciled before any retry.
8. **No automated Zerodha login.** Do not script passwords or TOTP.
9. **Never test against live Kite with real money in CI or by accident.** Live tests are manual, deliberate, tiny size, and approved by Imran.
10. Market facts (lot sizes, expiry days, freeze quantities, charges) are config or read from the instrument master. Never hard-code them.

---

## 4. Code quality rules

### General

1. No trading logic in the client API layer or the UI.
2. No broker calls from strategies. Strategies emit signals only.
3. No secrets in code, config committed to git, logs or the app bundle.
4. Handle loading, empty, error and disconnected states in the UI.
5. Do not introduce dependencies without a reason (see section 8).
6. Keep modules independently testable.

### Rust (engine)

- `cargo fmt` and `cargo clippy -- -D warnings` must be clean.
- No `unwrap()` or `expect()` on runtime data in the engine. Return errors.
- No allocation, locks or blocking IO on the hot thread.
- Use `tracing` for structured logs.

### Svelte and TypeScript (app)

- TypeScript strict mode.
- API message types are generated from Rust. Do not hand-write duplicates.
- The UI renders state from the engine. It does not compute P&L or risk itself.

### Python (research)

- `ruff` clean, `pytest` passing, type hints on public functions.
- Feature code must pass the Python vs Rust parity test.

---

## 5. Commands

Engine (from `engine/`):

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p esd -- sim                       # simulated day, API on 127.0.0.1:7400
cargo run -p esd -- sim --speed 0 --headless  # whole day, print summary
cargo run -p esd -- replay <file.esraw>       # replay a recorded raw log
cargo run -p esd -- device add <name>         # new app token (shown once)
```

Desktop app (from `app/`):

```powershell
npm install
npm run check
npm test
npm run tauri dev
npm run tauri build
```

Research (from `research/`):

```powershell
uv sync
uv run ruff check .
uv run pytest
```

Update this section when commands change.

---

## 6. Definition of done

A feature is done only when all of these are true:

- the behaviour works end to end
- tests exist for the important logic, and all tests pass
- format and lint are clean
- UI states (loading, empty, error, disconnected) exist where relevant
- no secrets are committed
- `docs/changelog.md` is updated
- `docs/currentstate.md` is updated if the state of the project changed
- changes are committed with a clear message
- changes are pushed to GitHub and the push is verified
- the working tree is clean

"Implemented" does not mean done. "Implemented, verified, committed and pushed" means done.

---

## 7. Git and GitHub workflow

### Push frequency

Push after every meaningful increment. Do not let unpushed work pile up.

Every increment follows this sequence:

```text
Implement
-> Test
-> Lint
-> Update docs/changelog.md (and docs/currentstate.md)
-> Commit
-> Push
-> Verify the push
-> Next increment
```

Examples of a meaningful increment: add the tick parser, add the daily max loss rule, add the paper broker, add the order log screen, fix reconnect handling. Each normally gets its own commit and push.

Do not:

- combine several unrelated features into one commit
- say "I'll push this later" or "I'll push everything at the end"
- end an increment with unpushed changes, unless Imran asked you not to push

Exception: when Imran asks to review something before it is pushed (for example a doc draft), commit locally if useful, and push only after his approval.

### Steps

```powershell
# after implementing
cargo test --all          # and app/research tests if touched
git status
git diff
# update docs/changelog.md
git add <specific files>
git commit -m "<conventional commit message>"
git push
git status
git log -1 --oneline
```

Stage specific files. Avoid `git add .` unless you have checked every file.

### Commit messages

Conventional commits with a scope:

```text
chore: bootstrap cargo workspace
feat(feed): parse full mode tick packets
feat(risk): add daily max loss rule
feat(oms): order state machine with reconcile
feat(app): kill switch button
fix(feed): resubscribe strikes after reconnect
refactor(market): split option chain from depth
test(risk): property tests for position limits
docs: update currentstate after paper broker
chore(release): bump version to 0.3.0
```

Never use meaningless messages such as `update`, `changes`, `fix`, `wip`, `stuff`, `latest`.

And again: **no "Co-authored-by" or any other AI attribution in commit messages.**

### If a push fails

Find the cause, fix it if safe, retry and verify. If it is blocked by something external (credentials, permissions, network, branch protection, conflicts), report it plainly:

```text
IMPLEMENTATION STATUS: COMPLETE
GITHUB PUSH STATUS: BLOCKED
REASON: <exact reason>
```

Never claim a push succeeded unless the command actually succeeded.

### Git safety

Never run these without Imran's explicit permission, if they could discard or rewrite work:

```text
git reset --hard
git clean -fd
git push --force
git push --force-with-lease
git rebase
git checkout -- <files>
git restore <files>
```

- Never rewrite published history.
- Never overwrite someone else's work.
- Work on the current branch unless told otherwise. Check `git branch --show-current` before pushing. If the branch looks wrong, stop and ask.

---

## 8. Dependency policy

Before adding a crate or package:

1. Confirm it solves a real problem.
2. Prefer mature, maintained packages.
3. Prefer small ones. This app aims to be lightweight.
4. Avoid packages that duplicate existing ones.
5. Note major dependencies and the reason in the changelog entry.

---

## 9. Changelog (`docs/changelog.md`)

The changelog is a mandatory project context file. Every meaningful increment updates it **in the same commit** as the work.

Newest entry first. Format:

```markdown
## YYYY-MM-DD (release: vX.Y.Z - short title)

### Added
- What was added, where (crate or folder), and the tests that cover it.

### Changed
- Behaviour that changed.

### Fixed
- What was broken, why, and how it was fixed.

### Verification
- cargo test: passed
- cargo clippy: clean
- other checks (replay run, manual test)

### Git
- Commits: `abc1234`
- Pushed to GitHub: yes
```

Use `## YYYY-MM-DD` without a release label for increments that are not a release. Leave out empty sections. Write for a reader who was not there: say what changed and why, plainly.

---

## 10. Commit attribution

No "Co-authored-by" details about any AI in any commit logs or messages. The same applies to "Generated with", "Assisted by" or similar lines in commit messages, PR descriptions, tags, release notes and the changelog. If a tool adds such a trailer automatically, remove it or turn that feature off.

---

## 11. Current state (`docs/currentstate.md`)

`currentstate.md` is the short "where are we now" file for picking up context quickly. Keep it under about one page. Update it whenever the state of the project changes (usually in the same commit as the changelog).

It holds:

- current phase and the active milestone from `docs/plan.md`
- what works now (short list)
- what is in progress
- next steps (ordered)
- open decisions and known issues
- last updated date and last commit

The changelog is history. Current state is a snapshot. Do not copy the changelog into it.

---

## 12. Versioning and releases

- Semantic versions: `0.MINOR.PATCH` until the first live-trading release, then `1.0.0`.
- Release commits use `chore(release): bump version to X.Y.Z`, and the tag is `vX.Y.Z`.
- The changelog heading for a release carries the version and a short title.

---

## 13. Secrets and logging

- Never commit `api_secret`, `access_token`, device tokens, AI gateway keys or `.env` files. They are in `.gitignore`.
- Server secrets live in a root-owned file on the VPS (see `docs/plan.md` section 12).
- Never log secrets or tokens. Log order requests without auth headers.

---

## 14. End of every work report

When reporting finished work, include:

```text
Implementation:
<what was implemented>

Verification:
<tests and checks run, with results>

Git:
Commit: <hash>
Push: SUCCESS | BLOCKED (reason)
Branch: <branch>
Working tree: clean | <what is left>
```

---

## 15. Writing style for docs

Plain and clear. Short sentences. No em dashes (use commas, periods or parentheses). No filler.
