# Fincept Integration — Implementation Progress

Tracks execution of `docs/fincept-integration-plan.md` (v2.1). Newest entry first.

---

## 2026-07-08 — Maintenance pass: Phase 1 wiring + World Markets UI

### Health

- `cargo check` (kalshi-monster `src-tauri`): **pass**
- `npm run build` (`src-ui`): **pass**

### Shipped this pass

| Item | Change |
|------|--------|
| Sidecar tracker | `fincept-sidecar/fincept_sidecar/engines/tracker.py` — category watchlists + chat snapshot tickers |
| Sidecar routes | `GET /api/v1/market/tracker`, `/tracker/{category}`, `/snapshot` in `routers/market.py` |
| Bridge HTTP | `FinceptBridge::get_json` for authenticated sidecar GETs |
| Chat context | `chat/fincept_context.rs` + wired into `send_message` and `send_message_stream` after Kalshi context |
| Tauri command | `get_fincept_market_tracker` registered in `lib.rs` |
| UI | **World markets** nav tab + `WorldMarketsView.tsx`; `finceptApi` in `services/tauri.ts` |

### Phase 0 note (forecast ↔ poller)

No code change required: `kalshi::grading::grade_pending_predictions` already calls `forecast::resolve_forecasts_for_market`, and `spawn_auto_grade_task` runs `resolve_pending_forecasts` when unresolved forecast rows exist.

### Still open (plan order)

1. Ledger / PASS logging columns (Phase 0 delta in progress doc 2026-07-07)
2. Expand tracker toward Appendix A (132 instruments)
3. Sidecar pytest: `tests/test_tracker.py` (3 tests, mocked quotes) — **pass** via `uv run pytest`
4. Settings UI hooks for `fincept_bridge_start_dev` / status (API exists; Settings panel not yet wired)

---

## 2026-07-07 — First implementation pass

### Gap analysis: the plan's Phase 0 is already substantially done

Plan v2.1 describes the app as "PrizePicks Monster v0.8.0" with hardcoded demo
props, a never-called `MarketContextBuilder`, dead `ml.rs`, and no forecast
tracking. The current repo has moved past that snapshot:

| Plan Phase 0 item | Current state |
|---|---|
| Live market board replacing demo props | ✅ Kalshi dashboard is the product (KalshiView, bootstrap strip, category stats) |
| Wire `MarketContextBuilder` | ✅/N-A — symbol no longer exists; chat context flows through `analysis/context.rs` |
| Delete or revive `ml.rs` | ✅ deleted; superseded by `ml_predictor.py` sidecar + Rust IPC (Phase 3 ML) |
| Forecast ledger | ◐ partial — `predictions` table tracks outcome/CLV/`entry_price`, and `eval_adapter` scores Brier via `edge-eval`; but the plan's `p_market`/`p_model`/`p_final`/`verdict` columns and PASS-logging do not exist yet |
| Resolution poller | ✅ `kalshi::grading::spawn_auto_grade_task` |

**Remaining Phase 0 delta:** extend the ledger with the shrinkage-pipeline
columns and start logging PASS verdicts (they're calibration data too, §4.3).

### Shipped: `edge_engine` module (Rust) — plan §4, §5.3, §6

`src-tauri/src/edge_engine/mod.rs`, registered in `lib.rs`. Pure math, no
I/O, no LLM, no sidecar dependency — the deterministic money path:

- Log-odds **shrinkage** toward the market price (λ default 0.25) — §4.1
- **Kalshi fee model** `0.07·P·(1−P)` with a key correction: Kalshi rounds
  the fee up to the next cent **per order**, not per contract, so edge math
  uses the unrounded per-contract fee and `order_fee()` ceilings the total
- **Net-edge verdicts** for both YES and NO sides (NO entry = `1 − yes_bid`
  plus fee at that price), 5¢ threshold with the 3¢ hard floor enforced in
  code (`effective_min_edge`), DO-NOT-TRADE flags forcing PASS — §4.2/§4.3
- **Deterministic aggregation** (§5.3): routing-weight × confidence log-odds
  pooling, disagreement penalty, per-agent attribution; returns `None`
  instead of dividing by zero when no agent opines
- **Sizing** (§6): binary Kelly `(p−c)/(1−c)`, quarter-Kelly default, named
  hard caps applied as explicit minimums with the binding cap reported
  ("capped by per-event limit"), never multiplied haircuts
- `AgentSignal` serde contract mirroring the sidecar's Pydantic schema, with
  a round-trip test against the exact JSON fixture the Python tests use

**Math corrections made while implementing** (the plan mandates the math be
exactly right, so these are pinned in tests):

1. §4.4 worked example states `p_final ≈ 0.756`; the exact value under the
   plan's own formula is **0.758922** (independently verified numerically).
   Net YES edge is **+2.5¢**, not +2.2¢. Verdict unchanged: PASS below 5¢.
2. Aggregation of the four §4.4 agent estimates reproduces `p_model = 0.8694`
   (plan: "~0.87") — confirms the routing-weight interpretation.

22 unit tests cover fee schedule/symmetry/order rounding, shrinkage golden
values and betweenness, aggregation identities and degenerate inputs,
end-to-end worked example, NO-side trades, flag/confidence gates, Kelly
anchors, and cap binding.

### Shipped: `fincept-sidecar/` scaffold — plan §7 Phase 1

New sibling directory `kalshi-build/fincept-sidecar/` (AGPL-3.0 + NOTICE from
day one; **must be split into its own public repo before any Fincept code
lands** — §3 Rule 1):

- FastAPI app factory with per-launch bearer-token auth (constant-time
  compare); every route authed — §10.2
- **Corrected startup handshake**: the plan's sketch read the bound port out
  of uvicorn server internals from a startup hook, which breaks across
  uvicorn versions. Implemented instead: bind `127.0.0.1:0` ourselves,
  `listen()`, print `FINCEPT_READY port=<n>`, hand the live socket to
  `Server.run(sockets=[...])`. Early connections queue in the backlog.
- Pydantic wire contracts (`AgentSignal`, `MarketOpinionRequest/Response`,
  `CatalystEvent`, `AssetSignal` for §14) with bounds enforced
  (probability ∈ [0.01, 0.99] or None — no hallucinated numbers)
- Market-data engine: original thin yfinance wrappers (permissive path,
  §3 Rule 5b) with §10.3 TTL caching and single-flight
- **15 tests, all passing** (Python 3.10): auth (401/200/constant-time
  construction), schema bounds, Rust-fixture round-trip, and a real
  subprocess handshake test that spawns `main.py`, reads the READY line, and
  makes authed requests against the announced port

### Also done

- Copied the plan into the repo: `docs/fincept-integration-plan.md`
  (scheduled runs could not reach the OneDrive Desktop copy)
- `docs/fincept-integration-groundwork.md` (2026-07-07 morning run) has the
  upstream Fincept v4 licensing/architecture verification

### Next up (in plan order)

1. **Ledger extension** (Phase 0 delta): add `p_market`/`p_model`/`p_final`/
   `verdict`/`verdict_reasons`/`agent_breakdown` columns + PASS logging;
   migration alongside the existing ALTER-TABLE pattern in
   `predictions/storage.rs`
2. **`FinceptBridge`** (Phase 1): Rust process supervisor via
   `tauri-plugin-shell` (already in Cargo.toml) — spawn, READY handshake
   with 30 s timeout, bounded restarts (3 per 10 min), degraded-mode flag,
   kill -9 degradation test
3. **Classification + routing table** (Phase 2): market category → agent
   weights (config, not code), feeding `edge_engine::aggregate`
4. **Fincept extraction spike** (§13.2, timeboxed 2 days): can the v4
   embedded-Python agent modules lift out cleanly, or fall back to v1.x tree
5. Ship `edge_engine` behind a Tauri command + Edge Board UI once agents
   produce real signals

### Notes / decisions

- Sandbox verification: sidecar pytest run in CI-like conditions here;
  Rust module compiled + tested via a standalone crate copy (results below
  in repo terms: `cargo test edge_engine::` once on Windows).
- Nothing committed to git; working tree carries the new files for review.
- `min_confidence` (0.30) is a provisional default the plan doesn't specify —
  flagged in config docs for re-fit once the ledger has data.
