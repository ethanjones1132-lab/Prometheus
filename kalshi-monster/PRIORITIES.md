# Kalshi Monster — Priority Roadmap

Last updated: 2026-07-07 (maintenance pass — Phase 0 forecast resolution bridge; 103 lib tests)

## Maintenance notes (2026-07-07, maintenance pass) — Phase 0: Forecast resolution bridge
- Wired the forecast ledger to Kalshi settlement: `resolve_forecasts_for_market` resolves all open rows per ticker with Brier scores.
- `grade_pending_predictions` now takes `db_pool` and resolves forecasts when prediction grading sees a settled market.
- `resolve_pending_forecasts` + auto-grade task poll when only forecast rows are pending (no prediction rows).
- `kalshi_grade_pending_predictions` IPC passes `db_pool` for manual grade parity.
- +1 lib test (`resolve_all_for_ticker`). **103** lib tests pass.
- Phase 0 status: resolution poller **complete** for forecasts + predictions; exit criteria met pending live settled data accumulation.

## Maintenance notes (2026-07-07, overnight cron — Phase 0 forecast ledger shipped; 102 lib + 20 vitest)
- Shipped the forecast ledger (`kalshi/src/kalshi/forecast.rs`) per the Fincept integration plan's Phase 0 schema: `forecasts` table with market ticker, timestamps, p_market/p_model/p_final, verdict (trade_yes/trade_no/pass), reasons, stake, agent breakdown, and resolution columns with Brier scores.
- Every `kalshi_record_paper_decision` now writes a forecast row — any opinion (YES, NO, or PASS) gets a row. This is the data every later phase (calibration, edge engine) depends on.
- Added +3 lib tests (insert, resolve+compute Brier, pass-outcome). 102 lib tests pass.
- Phase 0 status: 5 of 5 items done (market board live; `build_kalshi_context` wired; `ml_predictor` kept; forecast ledger shipped; resolution poller bridges forecasts on settlement).

Working copy: `C:\\Users\\ethan\\kalshi-build\\kalshi-monster`

Quick status: **P0 done · P1 done · P2 done · P3 1 pending**

---

## Maintenance notes (2026-07-06, afternoon cron pass) — Health verification

- Re-verified: `cargo check`, `tsc`, **99** lib tests, vitest **20**; working tree clean on `master`.
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no unblocked code work (clean-tree blocked-P3 policy).

## Maintenance notes (2026-07-06, cron pass) — Health verification

- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no unblocked code work (clean-tree blocked-P3 policy).
- Health: `cargo check`, `tsc`, **99** lib tests; vitest **20**; working tree clean on `master`.
- Reviewed `PRIORITIES.md` / `ROADMAP.md`: only open backlog item remains runtime data accumulation.

## Maintenance notes (2026-07-05, afternoon pass) — Planning doc sync

- **`ROADMAP.md`:** Maintenance cadence branch corrected to `master`; vitest noted in health checklist.
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no code change (per clean-tree blocked-P3 policy).
- Health: `cargo check`, `tsc`, **99** lib tests; vitest **20**; `agent-healthcheck.ps1` PASS.

## Maintenance notes (2026-07-05, maintenance pass) — Repo hygiene

- **`.gitignore`:** Ignore `**/predictions.db` so dev copies under `src-tauri/` are not committed (canonical DB remains `~/.openclaw/kalshi-monster/predictions.db`).
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live DB — no code change this pass.
- Health: cargo check, tsc, **99** lib tests; vitest **20** green.

## Maintenance notes (2026-07-04, maintenance pass) — Trading posture test coverage

- **`KalshiView.tradingPosture.test.ts`:** Unit tests for `tradingPostureFromTape` priority (in-progress > stale > snapshot > partial > full > warming).
- **`KalshiView.test.tsx`:** Integration test asserts **Stale tape** trading posture card when only stale `data_quality_notes` (no in-progress override).
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **12** green (6 posture unit + 6 component).

## Maintenance notes (2026-07-03, maintenance pass) — Trading posture tape hints

- **`KalshiView`:** `tradingPostureFromTape` drives the accent **Trading posture** card from `data_quality_notes` (in-progress refresh, stale tape, snapshot paint, partial/full) — same priority as decision tips.
- **Vitest:** Snapshot/stale/in-progress test asserts **Catalog updating** headline + body.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-03, maintenance pass) — In-progress refresh decision tip

- **`KalshiView`:** Insight rail decision tips mirror `data_quality_notes` when live catalog refresh is in progress (parity with stale/snapshot hints).
- **Vitest:** Extended snapshot/stale tape test to assert in-progress refresh tip.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-03, maintenance pass) — Dashboard tape quality hints

- **`commands/mod.rs`:** `build_kalshi_dashboard_data_quality_notes` adds stale-cache (>60s) and in-progress refresh hints on bootstrap; +2 unit tests.
- **`KalshiView`:** Decision tips mirror persisted-snapshot and stale-tape `data_quality_notes` (no new bootstrap struct fields).
- **Vitest:** Asserts snapshot/stale hints in status strip + insight tips.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard active sidecar CV

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `active_sidecar_models` via lightweight `per_category_models` parse from unified `_meta.json`; +1 unit test.
- **`KalshiView`:** Insight rail **Sidecar data** card lists active sidecars with samples and CV (Settings parity).
- **Vitest:** Asserts active sidecar line on bootstrap mock.
- Health: cargo check, tsc, **97** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-03, overnight pass) — Priority path review
- Reviewed the frontend hint path for dashboard snapshot accuracy.
- Decision: defer cross-exposure / stage-name fields until the older SQLite-WAL backend path is directly validated; reuse the existing `data_quality_notes` surface instead of expanding the struct prematurely.
- Health: cargo check, tsc, **97** lib tests pass; no new code committed beyond this planning note.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard unified ML CV

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `unified_cv_accuracy_mean/std` and `unified_trained_at` via lightweight `_meta.json` read in `phase3_dashboard_summary`; +1 unit test.
- **`KalshiView`:** Status strip ML artifacts label shows unified CV ± std; insight rail shows trained date and ROADMAP data-metric line when ready.
- **Vitest:** CV and trained-date assertions on bootstrap mock.
- Health: cargo check, tsc, **96** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard sidecar category progress

- **`MLPhase3DashboardSummary`:** Adds `non_sports_category_stats` (Politics/Economics/Weather graded counts) in bootstrap SQL path.
- **`KalshiView`:** Insight rail **Sidecar data (Kalshi paper)** card mirrors Settings per-category progress without opening Settings.
- **Vitest:** Asserts sidecar insight card when bootstrap includes category stats.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard ML artifact hints

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `unified_model_on_disk` / `active_sidecar_count` via `ml_artifacts_on_disk_summary` in `phase3_dashboard_summary`.
- **`KalshiView`:** Status strip shows ML artifacts; insight rail tips for pending grades and next sidecar unlock.
- **Vitest:** Artifact label assertion on bootstrap mock.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard Train ML CTA

- **`KalshiView`:** When `ml_phase3.auto_retrain_eligible`, status strip shows **Train ML models** (`ml_train_model` IPC, refreshes bootstrap, flashes sample/CV summary).
- **`KalshiView.test.tsx`:** Vitest for dashboard ML train action (4 tests).
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard grade pending CTA

- **`KalshiView`:** When `ml_phase3.kalshi_pending_predictions > 0`, status strip shows **Grade N pending** (calls `kalshi_grade_pending_predictions`, refreshes bootstrap, flashes W/L/PnL summary).
- **`index.css`:** Compact `.smallGradeBtn` styling in `.dashboardStatus`.
- **Vitest:** New test for dashboard grade action.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-06-30, maintenance pass) — Dashboard Phase 3 auto-retrain hint

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `auto_retrain_eligible` / `resolved_until_auto_retrain` (total resolved SQL in bootstrap); `build_phase3_dashboard_summary` extended; test assertions updated.
- **`KalshiView`:** Diagnostic strip shows pending Kalshi grades and auto-retrain readiness (parity with Settings ML card).
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-06-30, maintenance pass) — Kalshi dashboard Phase 3 hint

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary`, `phase3_dashboard_summary` / `build_phase3_dashboard_summary` (SQL-only, no joblib read); +1 unit test.
- **`kalshi_get_dashboard_bootstrap`:** Injects `ml_phase3` via `db_pool` (one IPC for markets + ML readiness).
- **`KalshiView`:** Diagnostic strip shows sidecar progress, resolved Kalshi paper rows, next category unlock; Vitest extended.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView tests pass.

## Maintenance notes (2026-06-30, maintenance pass) — Phase 3 category scope

- **`ml_predictor.rs`:** `fetch_category_stats` counts only Kalshi paper rows (`$.ticker` in `full_decision_json`); `KALSHI_TICKER_PREDICATE` constant; LLM header adds Kalshi journal line when Phase 3 incomplete; header test extended.
- **Settings UI:** Clarifies per-category list is Kalshi ticker rows only (mixed `predictions.db` totals unchanged on unified card).
- Health: cargo check, tsc, **94** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 ROADMAP visibility

- **`ml_predictor.rs`:** `phase_3_data_metric_ready`, `kalshi_resolved_predictions` / `kalshi_pending_predictions` on `MLModelStatus`; LLM header shows Phase 3 progress when incomplete; +2 unit tests.
- **Settings UI:** ROADMAP data metric badge; Kalshi-only resolved/pending line vs mixed `predictions.db` totals.
- Health: cargo check, tsc, **94** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 ML prompt + DB hygiene

- **`ml_predictor.rs`:** `format_ml_training_header` (CV ± std, active sidecars) for chat prompts; `DROP INDEX IF EXISTS idx_ml_pred_ticker` on ML table init; +1 unit test.
- **`enhanced_prompt.rs` / `openrouter.rs`:** Shared ML header helper (DRY).
- **Settings UI:** Unified model CV shows ± std when `_meta.json` provides it; TS types extended.
- Health: cargo check, tsc, **92** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 auto-retrain UX

- **`ml_predictor.rs`:** `auto_retrain_eligible` / `resolved_until_auto_retrain` on `MLModelStatus`; helpers + unit test; removed invalid `idx_ml_pred_ticker` index (no `ticker` column on `ml_predictions`).
- **Settings UI:** Shows when auto-retrain after grading is active vs how many resolved rows are still needed (≥10 gate).
- Health: cargo check, tsc, **91** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — Phase 3 category visibility

- **`ml_predictor.rs`:** `ensure_non_sports_sidecar_stats` merges placeholder Politics/Economics/Weather rows when DB has no graded rows in those categories; `nearest_non_sports_sidecar_unlock` + `next_sidecar_*` on `MLModelStatus`; +2 unit tests.
- **Settings UI:** “Next sidecar unlock” line when a target category is still below 10 graded.
- Health: cargo check, tsc, **90** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — Phase 3 progress UX

- **`ml_predictor.rs`:** `trainable_non_sports_categories` / `non_sports_sidecar_target` on `MLModelStatus`; `count_trainable_non_sports_categories` + unit test; debug log when auto-retrain skipped (<10 resolved).
- **Settings UI:** Phase 3 progress line (X/3 Politics/Economics/Weather); clarified ≥10 total graded rows for auto-retrain.
- **`ROADMAP.md`:** Notes Settings tracks the ≥3-category data metric.
- Health: cargo check, tsc, **88** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — P3 ML polish

- **`ml_predictor.rs`:** Auto-retrain after grading only when ≥10 resolved rows (`should_retrain_given_resolved`); passes DB pool from auto-grader; +1 unit test.
- **Settings UI:** Active sidecars show per-model CV accuracy when present in `_meta.json`.
- **`ROADMAP.md`:** Marked code-complete Phase 3 success metrics (sidecar listing, predict routing, CV/mix visibility).
- Health: cargo check, tsc, lib tests.

## Maintenance notes (2026-06-27, maintenance pass) — ML Settings visibility + ROADMAP

- **`ROADMAP.md`:** Created phased plan (P0–P4) with Phase 3 ML success metrics; complements `PRIORITIES.md`.
- **Settings UI:** Auto-retrain helper text; show `trained_at` on unified model card; surface `training_category_breakdown` from `_meta.json`.
- **`ml_predictor.rs`:** `should_retrain_after_grading` gate + unit test.
- Health: cargo check, tsc, lib tests.

## Maintenance notes (2026-06-27, maintenance pass) — P3 ML training loop

- **`ml_predictor.rs`:** `retrain_after_grading` — background unified + sidecar retrain after new grades land.
- **`kalshi/grading.rs`:** Auto-grader spawns ML retrain when `graded > 0` (non-blocking).
- **Settings UI:** **Train unified + sidecar models** + **Refresh status** on ML readiness card (`ml_train_model` IPC).
- Health: cargo check, tsc, **85** lib tests pass.

## Maintenance notes (2026-06-27, maintenance pass) — Kalshi notification prefs wired

- **`kalshi/grading.rs`:** Auto-grader respects `notification_settings.json` — skips win/loss alerts when `kalshi_notifications_enabled` or master `enabled` is off; grading summary gated by `grading_complete_enabled`.
- **`notification.rs`:** Helpers + backward-compatible deserialize for missing `kalshi_notifications_enabled` (defaults on); 3 unit tests.
- **Settings UI:** Toggle **Kalshi market resolved alerts** loads/saves via notification IPC.
- Health: cargo check, tsc, **85** lib tests pass.

## Maintenance notes (2026-06-27, overnight pass) — Kalshi market resolution notifications

- **`notification.rs`:** Added `KalshiMarketWin`, `KalshiMarketLoss` variants to `NotificationType`, `kalshi_notifications_enabled` setting.
- **`kalshi/grading.rs`:** `spawn_auto_grade_task` now accepts AppHandle + DB pool; emits per-prediction Win/Loss notifications and a GradingComplete summary when the auto-grader resolves markets.
- **`lib.rs`:** Passes AppHandle + db_pool to auto-grader.
- You'll now see a notification pop up when a Kalshi paper prediction market resolves (Win ❌ / Loss ✅) with title, ticker, stake, and PnL.
- Health: cargo check, tsc, **82** lib tests pass.

## Maintenance notes (2026-06-26, evening pass) — P3 readiness UX + Phase 4 polish

- **`MLCategoryStats`:** `samples_until_trainable` + `min_resolved_for_sidecar` for Settings progress (e.g. `3/10 graded, 7 more for sidecar`).
- **Dashboard bootstrap:** notes when tape is still the SQLite rehydrate (`showing_persisted_snapshot`) before live refresh completes.
- **`market_cache_store`:** async SQLite roundtrip test for save/load.
- Health: cargo check, tsc, **82** lib tests pass.

## Maintenance notes (2026-06-26, 4pm pass) — Dashboard Phase 4 (SQLite persistence)

- **`kalshi_market_cache` table:** JSON snapshot of last quick/full cache in `predictions.db` (`market_cache_store.rs`).
- **Startup:** `load_persisted_cache` rehydrates `KalshiClient` + `SharedCache` when snapshot age ≤ 24h; API refresh still runs when in-memory TTL (60s) is stale.
- **After fetch:** `store_cache` async-persists to SQLite on every quick/full warm.
- Health: cargo check, tsc, **81** lib tests pass.

## Maintenance notes (2026-06-26, 4pm pass) — Dashboard Phase 4 (partial)

- **Startup quick-cache prefetch:** `lib.rs` spawns `ensure_quick_cache()` immediately on app setup; full catalog warm still runs after 8s idle.
- **Market detail:** 300ms debounce on `computeStakeAdjustment` IPC while editing stake/side (Phase 3 frontend trim).
- Health: cargo check, tsc, **80** lib tests pass, MarketDetailPanel vitest green.

## Maintenance notes (2026-06-26) — Dashboard Phase 2 (shared cache decoupling)

- `Arc<RwLock<Option<KalshiCache>>>` (SharedCache) so cache writes populate both `KalshiClient.cache` + `shared_cache`.
- `FetchInProgressGuard` (AtomicBool) prevents stacked full-catalog warm cycles.
- `kalshi_get_cache_state` Tauri command reads cache state without locking the client mutex.
- `KalshiClient::new()` now accepts `shared_cache: Arc<RwLock<Option<KalshiCache>>>`.
- Managed as Tauri state: `.manage(kalshi_cache_holder)`.
- Health: cargo check, tsc, **80** lib tests pass.
- Committed as `feat(kalshi): decouple KalshiCache into shared Arc<RwLock> for lock-free reads`.

## Maintenance notes (2026-06-23)
- Fixed `unused import: sqlx::sqlite::SqlitePoolOptions` in `src-tauri/src/predictions/tracker.rs` (was test-only; moved use into `#[cfg(test)] mod tests`)
- Verified: cargo check clean, 78 lib tests pass, UI tsc clean.
- P3 items remain blocked pending accumulated graded data in predictions.db
- Working tree was clean at start of pass; no remote configured so no push.

## Maintenance notes (2026-06-24)
- Wired `compute_historical_brier` (from graded Win/Loss predictions in predictions.db), `refresh_historical_brier` Tauri command, and UI trigger in SettingsView.tsx. `VolatilityAdjustedKelly` strategy (with `volatility_adjusted_kelly` fn) now uses real `historical_brier` for auto-shrinkage when graded history exists. (P3 brier support complete; strategy was in prior commit)
- Committed changes from maintenance pass (no remote, skipped push).
- Re-ran health checks post-commit: cargo check, tsc, 78 tests all green.

## Maintenance notes (2026-06-25, evening pass)
- `ml_predictor.py`: trains optional sidecar models for Politics/Economics/Weather when each has 10+ graded samples; `predict_batch` routes to sidecar when present.
- `ml_predictor.rs`: `MLPerCategoryModel` + `per_category_models` on `MLModelStatus` (from `_meta.json` + on-disk joblib check).
- Settings: **ML multi-category readiness** card (`ml_get_model_status`, per-category resolved counts and sidecar status).
- `.gitignore`: `__pycache__` / `*.pyc`.
- Health: cargo check, tsc, **80** lib tests pass.

## Maintenance notes (2026-06-25, afternoon pass)
- Completed Rust wiring for `category_code` on `MLPrediction` (predict JSON + prompt context).
- Python: shared `CATEGORY_MAP`, training `category_breakdown` in `_meta.json` and train response.
- Rust: `MLCategoryStats` + `fetch_category_stats` (SQLite `json_extract` on `full_decision_json`); `MLModelStatus.category_stats` / `training_category_breakdown`; readiness text in status message.
- `enhanced_prompt.rs`: non-sports ML rows show `[cat:N]` when category_code > 0.
- Health: cargo check, tsc, **80** lib tests pass.

## Maintenance notes (2026-06-25)
- Extended `ml_predictor.py` (extract_features_from_db + predict_batch) to support Kalshi predictions: now queries rows with full_decision_json, parses category/fair_probability_pct/edge_points/liquidity etc into category_code + shared numeric features. Sports path unchanged. Enables P3 multi-category ML (politics/econ/weather) once graded Kalshi history accumulates.
- Updated docstring, FEATURE_COLUMNS (added category_code), export, and both train/predict paths.
- Health checks remain green (cargo check, tsc, 78 tests).
- Working tree was clean at start; changes committed below.

---

## High-impact improvements (ranked)

| Priority | Item | Why it matters | Status |
|----------|------|----------------|--------|
| **P0** | Fix grading to use `contract_side` + store `market_price_at_entry` | Unblocks trustworthy paper-sim and the entire calibration loop | ✅ Done |
| **P0** | Background auto-grade for Kalshi (poll resolved markets) | Notifications auto-grade ESPN props only; Kalshi grading was manual | ✅ Done |
| **P1** | Correlated position auto-scaling | Warnings exist (event/series co-exposure) but Kelly stakes were not scaled down | ✅ Done |
| **P1** | Wire `edge_eval` calibrator into Kalshi decision path | Isotonic calibrator applied to `analyze_single_prop` (sports props), not LLM `KalshiTradeDecision` forecasts | ✅ Done |
| **P1** | Kalshi historical price/spread snapshots | `line_tracker.rs` is PrizePicks-only; no candlestick API in `kalshi/client.rs` — blocks CLV tracking and momentum signals | ✅ Done |
| **P1** | Kalshi-native correlation engine | `correlation.rs` is NFL prop families; portfolio checks were ticker-prefix heuristics. Now a native correlation cluster graph links distinct series by shared macro/political driver | ✅ Done |
| **P2** | Persist `localMaxBetPct` to config | Now a persisted `max_bet_pct` config field, read/written by SettingsView + MarketDetailPanel | ✅ Done |
| **P2** | Sync bankroll limits from `predictions.db` + paper positions | Makes daily/weekly cap warnings and `BankrollView` accurate | ✅ Done |
| **P2** | Model disagreement flags at entry | Flag when `fair_probability_pct` diverges sharply from market implied prob at decision time | ✅ Done |
| **P2** | CLV per prediction | Grading records close price and CLV on paper predictions | ✅ Done |
| **P3** | Volatility-adjusted Kelly from historical Brier | Shrinkage slider is manual; handoffs call for Brier-driven auto-shrinkage | ✅ Done (2026-06-24; brier compute/refresh/strategy wired) |
| **P3** | Multi-category ML classifiers (politics/econ/weather) | Current ML is scikit-learn on sports prop features via Python subprocess; README still lists ML training as unchecked | ⬜ In progress (2026-06-27; auto-retrain after Kalshi auto-grade + Settings train button; sidecar trainers when 10+ graded/category; awaits graded Kalshi history) |

---

## Remaining count

| Tier | Done | Remaining |
|------|------|-----------|
| P0 | 2 | **0** |
| P1 | 4 | **0** |
| P2 | 4 | **0** |
| P3 | 1 | **1** |

**1 item left** (Multi-category ML classifiers — now in progress). VolatilityAdjustedKelly brier support shipped 2026-06-24. Plus the off-roadmap notification-settings persistence fix (now shipped).

---

## P0 implementation notes (shipped)

- `src-tauri/src/kalshi/grading.rs` — contract-side grading, binary PnL, `grade_pending_predictions`, `spawn_auto_grade_task`
- `src-tauri/src/kalshi/models.rs` — `contract_side`, `market_price_at_entry` on predictions
- `src-tauri/src/predictions/tracker.rs` — rich `KalshiTradeDecision` extraction
- `src-tauri/src/lib.rs` — auto-grade task on startup

---

## P2 implementation notes (shipped)

- `src-tauri/src/bankroll.rs` — async `get_bankroll_summary_synced`, `apply_bankroll_cap`, prediction/paper exposure aggregation
- `src-tauri/src/commands/mod.rs` — bankroll-aware stake adjustment and paper decision capping
- UI: `src-ui/src/components/SettingsView.tsx`, `src-ui/src/components/KalshiPredictionsPanel.tsx`
- `src-tauri/src/config.rs` — `max_bet_pct` persisted config field (resolves the `localMaxBetPct` item); `MarketDetailPanel.tsx` writes it via config save

**P2 remaining:** none.

---

## P1 implementation notes (shipped)

- `src-tauri/src/kalshi/portfolio_risk.rs` — Kelly scaling (event 0.50, series 0.75, **cluster 0.82**, category 0.90, same-ticker 0.85)
- `src-tauri/src/analysis/calibration.rs` — isotonic calibrator wired into Kalshi paper trades
- `src-tauri/src/kalshi/price_tracker.rs` — snapshots on `kalshi_refresh`, `kalshi_get_price_history`
- UI: `src-ui/src/components/KalshiView.tsx`, `MarketDetailPanel.tsx`, `KalshiPredictionsPanel.tsx`, `PriceHistoryChart.tsx`

**P1 native correlation graph (shipped 2026-06-22):** `CorrelationStrength::Cluster` + `CORRELATION_CLUSTERS` map in `portfolio_risk.rs` links distinct series sharing a macro/political driver (`us-rates-inflation`: CPI/PCE/Fed/payrolls/GDP; `us-federal-politics`: president/senate/house/party-control). Conflict explanations name the driver. The cluster map is the extension point for future event-graph edges.

---

## Suggested next target: P3

P0–P2 are complete. 

1. Volatility-adjusted Kelly from historical Brier (auto-shrinkage) — ✅ Done (2026-06-24; `volatility_adjusted_kelly` fn + `compute_historical_brier` + `refresh_historical_brier` command + UI trigger wired; strategy now uses real data for shrinkage when graded history accumulates in predictions.db)
2. Multi-category ML classifiers (politics/econ/weather) — ⬜ In progress (2026-06-25; sidecar train/infer wired; UI readiness in Settings; fully active once politics/econ/weather each accumulate 10+ graded rows)

Off-roadmap fix shipped 2026-06-22: notification settings now persist to `~/.openclaw/kalshi-monster/notification_settings.json` (`notification::load_settings`/`save_settings`); previously `save_notification_settings` only logged and `get_notification_settings` always returned defaults.

---

## Dashboard performance (deferred)

**Phase 1 (shipped 2026-06-17):** flat `GET /markets` quick cache (replaces nested `/events` for dashboard load). See `kalshi/client.rs` — `fetch_markets_flat_pages`, `ensure_quick_cache`.

### Phase 2 — Decouple cache reads from long fetches ✅ Done (2026-06-26)

- Extract `Arc<RwLock<KalshiCache>>` + `fetch_in_progress` guard so UI reads never block on 20-page full warm ✅
- Background full-catalog warm writes cache without holding the outer `KalshiClient` mutex across HTTP pagination ✅
- Add `kalshi_get_cache_state` Tauri command (read-only, no client lock) ✅
- Optionally slim cache to `KalshiMarketSummary` instead of full `KalshiMarket`
- **Target:** warm revisit under 300ms; category switch under 500ms

### Phase 3 — Frontend critical-path trim (shipped 2026-06-23)

- Keep `KalshiView` mounted across tab switches (avoid cold reload)
- Combined IPC: `kalshi_get_dashboard_bootstrap` → `{ markets, categories, cache_full }` ✅ Shipped
- Show partial-cache indicator when `full_catalog == false` ✅ Shipped (cacheLabel/partialCatalog in KalshiView)
- Defer `KalshiPredictionsPanel` load; debounce `computeStakeAdjustment` in market detail ✅ Shipped (predictions deferred via `marketsReady`; stake debounce 300ms in MarketDetailPanel)
- Calibration status inline display in MarketDetailPanel ✅ Shipped

### Phase 4 — Startup prefetch and persistence (optional)

- Prefetch quick cache at app startup (before user opens dashboard) ✅ Shipped (2026-06-26)
- Delay full warm until quick cache exists + idle window (or explicit Refresh only) ✅ (quick prefetch + 8s delayed full warm)
- Persist summary cache to SQLite for instant next-launch paint ✅ Shipped (2026-06-26; `kalshi_market_cache` + startup rehydrate)

---

## Environment notes

- Canonical WSL repo (`~/.openclaw/agents/coderclaw/workspace/kalshi-monster`) was unreachable as of 2026-06-17
- `edge-eval` and `monster-edge-core` live at `C:\\Users\\ethan\\kalshi-build\\` (sibling paths)
