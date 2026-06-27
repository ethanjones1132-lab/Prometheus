# Kalshi Monster — Priority Roadmap

Last updated: 2026-06-27 (maintenance — ML retrain after auto-grade + Settings train button; health green, 85 tests)

Working copy: `C:\\Users\\ethan\\kalshi-build\\kalshi-monster`

Quick status: **P0 done · P1 done · P2 done · P3 1 pending**

---

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
