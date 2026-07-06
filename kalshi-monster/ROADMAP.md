# Kalshi Monster — Roadmap

Last updated: 2026-07-06

Canonical backlog detail lives in `PRIORITIES.md`. This document is the phased plan and success metrics.

## Vision

Desktop research app for Kalshi prediction markets: calibrated LLM + paper journal, portfolio risk, and ML-assisted forecasts — **no live order execution**.

## Phase status

| Phase | Theme | Status |
|-------|--------|--------|
| **0** | Trustworthy grading & paper loop | ✅ Complete (P0) |
| **1** | Portfolio risk, calibration, Kalshi-native data | ✅ Complete (P1) |
| **2** | Bankroll sync, CLV, disagreement flags, config | ✅ Complete (P2) |
| **3** | Brier-driven Kelly + multi-category ML | 🔄 In progress (Brier ✅; ML awaits graded history) |
| **4** | Dashboard performance & launch polish | ✅ Mostly complete (SQLite cache, shared cache, prefetch) |

## Phase 3 — Multi-category ML (active)

**Goal:** Sidecar classifiers for politics, economics, and weather when each category has ≥10 graded Kalshi rows in `predictions.db`.

**Shipped:**

- Python `ml_predictor.py` unified + sidecar training; Rust IPC (`ml_train_model`, `ml_get_model_status`, `predict_batch`)
- Settings readiness card (per-category counts, manual train, auto-retrain after Kalshi auto-grade)
- Dashboard bootstrap strip: sidecar progress, Kalshi journal counts, pending grades, auto-retrain gate, next category unlock
- Dashboard insight rail: per-category Politics/Economics/Weather graded progress (same data as Settings ML card)
- Dashboard strip shows on-disk unified/sidecar ML artifact counts (path existence, no metadata parse)
- Dashboard strip includes unified model CV ± and trained date from `_meta.json` when available
- Dashboard insight rail lists active sidecars with per-model CV from `_meta.json` (Settings parity)
- Dashboard **Train ML models** button when ≥10 resolved rows (manual retrain without opening Settings)
- LLM system prompts list active sidecars + CV ± when model metadata is available (`format_ml_training_header`); incomplete Phase 3 data progress appended when &lt;3 categories ready
- Settings: `phase_3_data_metric_ready`, Kalshi-only graded counts vs mixed DB totals; per-category breakdown scoped to Kalshi ticker rows
- `category_code` on ML predictions; enhanced prompts show `[cat:N]` for non-sports

**Success metrics:**

- [ ] ≥3 non-sports categories each with 10+ resolved rows in DB (tracked in Settings as Phase 3 progress)
- [x] Sidecar joblib files on disk and listed in Settings “Active sidecars” (when trained)
- [x] `predict_batch` routes non-sports rows through sidecars when present (Python)
- [x] CV accuracy and training mix visible in Settings after train (unified + per-sidecar CV)

**Blocker:** Accumulated graded Kalshi paper history (user/runtime data, not code).

## Phase 4 — Performance (deferred / optional)

Targets from `PRIORITIES.md`: warm revisit &lt;300ms, category switch &lt;500ms. Phase 2–4 dashboard items are largely shipped; further slimming of cache payloads is optional.

## Maintenance cadence

- Twice-daily cron on `master` (health: `cargo check`, UI `tsc`, lib tests, vitest)
- Agent skills: `kalshi-maintain`, `kalshi-ship-next` (see `AGENTS.md`)

## Related repos

- `monster-edge-core`, `edge-eval` — sibling paths under `kalshi-build/`
- User data: `~/.openclaw/kalshi-monster/` (predictions.db, ML models, notification settings)