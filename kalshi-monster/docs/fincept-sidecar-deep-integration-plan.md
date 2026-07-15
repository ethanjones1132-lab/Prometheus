# Plan: Deepen Fincept Sidecar Integration

> **Superseded for sequencing:** use **`docs/MASTER-PLAN.md`** for chronological implementation order (this plan is still valid for Fincept-only detail). Phase A is **done**; remaining work maps to MASTER sprints 1–4 and 7.

## Context

The Fincept sidecar is **scaffolded but critically underused** relative to the integration plan (`docs/fincept-integration-plan.md` v2.1).

### What works today

| Layer | Status | Where |
|-------|--------|--------|
| Process lifecycle | ✅ Spawn, READY handshake, bearer auth, restart budget | `src-tauri/src/fincept_bridge/mod.rs` |
| Market snapshot / tracker | ✅ yfinance world markets | `fincept-sidecar/routers/market.py`, World Markets UI |
| Chat cross-asset injection | ⚠️ Query-gated only (`needs_cross_asset_context`) | `chat/fincept_context.rs` |
| Agent HTTP | ⚠️ `POST /api/v1/agents/market-opinion` | `agents/orchestrator.py` |
| Live agents | ⚠️ **Only 2 of 9** | `technical` (yfinance), `contract_tape` (mids) |
| Stub agents | Always `probability=None` | macro, news, sentiment |
| Edge pipeline | ✅ Sidecar → aggregate → shrink → ledger | `edge_engine/pipeline.rs` → `analyze_and_log_forecast` |
| Entry point | ⚠️ Manual **Analyze** only | `kalshi_analyze_market_edge` / Calibration |

### Why it feels underutilized (evidence from ledger + code)

1. **Analyst chat never calls agents.** `commands/chat.rs` injects Kalshi tape + optional Fincept *spot snapshot* + web. It does **not** call `market-opinion`. LLM invents fair values alone.
2. **Agent weights are dead for top categories.** Routing gives **macro 50%** on economic, **news 35%** on politics (`pipeline.rs`), but those agents always return `None` → `p_model` often missing → all-PASS ledger rows with empty agent opinions.
3. **Technical rarely opines.** Needs `underlying_ticker` + `strike` + horizon in context. Most Kalshi titles never get that inference from Rust.
4. **Contract tape starved.** Needs `contract_mids` history; Analyze path can pass them, but chat path and many batch calls send empty series.
5. **Two forecast writers, one without agents.** Chat extract writes LLM fairs to `forecasts` without sidecar breakdown; edge pipeline writes agent-aware rows. Calibration flywheel underfeeds real `p_model`.
6. **Plan Phase 2 incomplete.** EconDB/macro, news feeds, depth tiers (quick/standard/deep), Edge Board UI, per-agent debug endpoints unused.

**Constraint (unchanged):** AGPL boundary stays process/HTTP only. No Python imports in Rust. Agents return honest `probability=None` rather than invented numbers. No Kelly/EV formula rewrites in this plan (wiring + agent data only).

---

## Recommended approach

**Make the sidecar the default probability co-pilot for every serious market look**, not a manual Analyze button + World Markets tab.

Priority order (highest leverage first):

```
P0  Wire agents into Analyst + feed mids/underlying into every opinion call
P1  Honest new agents with real data (news reuse, macro series, better technical mapping)
P2  Product surface (Edge Board, agent drawer, depth tiers)
P3  AGPL hygiene + optional Fincept module extraction
```

---

## Critical files

### Rust (app)

| File | Role |
|------|------|
| `kalshi-monster/src-tauri/src/fincept_bridge/mod.rs` | `get_json` / `post_json` (reuse) |
| `kalshi-monster/src-tauri/src/edge_engine/pipeline.rs` | `analyze_and_log_forecast`, routing weights, category map |
| `kalshi-monster/src-tauri/src/commands/kalshi_analysis.rs` | Analyze IPC entry |
| `kalshi-monster/src-tauri/src/commands/chat.rs` | Chat path — **main integration hole** |
| `kalshi-monster/src-tauri/src/chat/fincept_context.rs` | Snapshot only today |
| `kalshi-monster/src-tauri/src/chat/kalshi_context.rs` | Retrieved markets / open set |
| `kalshi-monster/src-tauri/src/kalshi/price_tracker.rs` | Mid history for `contract_mids` |
| `kalshi-monster/src-tauri/src/kalshi/forecast.rs` | Ledger |

### Sidecar

| File | Role |
|------|------|
| `fincept-sidecar/agents/orchestrator.py` | Fan-out |
| `fincept-sidecar/agents/technical.py` | Price-level P(S>K) |
| `fincept-sidecar/agents/contract_tape.py` | Mid-path signal |
| `fincept-sidecar/fincept_sidecar/schemas.py` | Wire contracts |
| `fincept-sidecar/fincept_sidecar/engines/market_data.py` | yfinance |
| `fincept-sidecar/fincept_sidecar/routers/agents.py` | HTTP |

### UI

| File | Role |
|------|------|
| `src-ui/src/components/CalibrationView.tsx` | Analyze today |
| `src-ui/src/components/ChatView.tsx` / Command desk | Should surface agent priors |
| Settings Fincept card | Lifecycle only |

### Reuse (do not reinvent)

- `FinceptBridge::post_json("/api/v1/agents/market-opinion", body)`
- `edge_engine::pipeline::analyze_and_log_forecast` / `AnalyzeMarketInput`
- `price_tracker` snapshots → mid series
- `chat::web_context` for news-ish evidence (repackage as agent, not second LLM path)
- Existing `AgentSignal` aggregate/evaluate/shrink in `edge_engine` (**no math changes**)

---

## Implementation plan

### Phase A — Close the wiring gap (highest ROI, days)

**A1. Shared “opinion request builder” in Rust**

Extract from `pipeline.rs` a helper used by Analyze **and** chat:

```text
build_opinion_input(client, ticker) -> AnalyzeMarketInput
  - title, rules, close, category, yes_bid/ask from tape
  - contract_mids from price_tracker (last N snapshots, 0–1)
  - underlying_ticker / strike inferred from title + series map
```

**A2. Analyst chat: agent priors for open retrieved markets**

In `commands/chat.rs` after `build_kalshi_context_full`:

1. Take up to **3** `open_markets` (gate OPEN, prefer tight spread / volume).
2. For each, call sidecar `market-opinion` **or** full `analyze_and_log_forecast` (prefer full path so ledger + agents stay unified).
3. Append a compact block to system context, e.g.:

```text
## SIDECAR MODEL PRIORS (Fincept agents — not final Kelly)
- [TICKER] p_market=0.64 p_model=0.58 p_final=0.61 conf=0.22 verdict=pass
  agents: technical=0.55@0.3, contract_tape=0.60@0.2, macro=null, news=null
  Use as prior; do not ignore gates/rules. Prefer PASS if agents silent.
```

4. Cap latency: parallel requests, 2–3s timeout each, skip if bridge offline (existing degraded path).

**A3. Stop dual ledger semantics for chat**

Replace chat-only LLM forecast insert with:

- Prefer edge pipeline result when available.
- If LLM decision differs, store LLM fair in `agent_breakdown.llm` or notes — **do not invent a second `p_model` formula**.

**A4. Always populate contract_mids on Analyze**

Ensure `kalshi_analysis.rs` loads price snapshots for every ticker (today often empty → contract_tape null).

**Acceptance A**

- Chat “mispriced today” with bridge online produces SIDECAR MODEL PRIORS for ≥1 market when mids exist.
- New forecast rows from Analyze/chat show non-null `agent_breakdown` with real signals when data exists.
- Offline bridge: chat still works; card says “sidecar offline”.

---

### Phase B — Make more agents actually opine (honest data only)

**B1. Technical coverage**

- Expand `UNDERLYING_TICKERS` + Rust-side inference for Kalshi series (`KXBTCD`, `KXETHD`, `KXINX`, weather underlyings if mappable).
- Parse strike from titles (`Will Bitcoin be above $100k…`, `S&P above 5500`).
- Pass `horizon_days` from close_time.

**B2. News agent (reuse existing web path)**

- New `agents/news.py`: resolution-aware heuristic over structured snippets (not free hallucination).
- Inputs: title + rules + optional Brave/DDG hits from Rust `context.web_snippets`.
- Output: `probability=None` unless evidence clearly shifts base rate; always fill `catalysts` when dates found.
- This reuses `chat/web_context` instead of a new LLM in the sidecar.

**B3. Macro agent (economic contracts)**

- Prefer **public series** before Fincept EconDB extract:
  - FRED API (optional key) or BLS public JSON for CPI / unemployment / payrolls proxies.
- Opine only when category is `economic` and series maps cleanly to contract.
- Else `probability=None` with rationale “no series mapping”.

**B4. Sentiment**

- Defer unless a free, non-AGPL feed exists; keep stub until then (plan honesty rule).

**B5. Routing honesty**

- When macro/news always None, optionally log “weight mass on silent agents” in verdict reasons (already partially true). Do **not** reweight silently without config — surface silence in UI.

**Acceptance B**

- On a BTC daily / index level market with history: technical often non-null.
- On a Fed/CPI market with FRED keyed: macro non-null when mapped.
- Politics still mostly null until news path is solid — PASS stays correct.

---

### Phase C — Product surface (use what you have)

**C1. Edge Board (Command desk or Calibration)**

- Rank open markets by |edge_net| after batch analyze (top N from tape, rate-limited).
- Columns: ticker, p_market, p_model, p_final, verdict, agents opining, confidence.
- Click → agent breakdown drawer (signals already in ledger JSON).

**C2. Analyst UI**

- Chip: “Sidecar online · N agents opining on context”.
- Optional “Deep analyze top 3” button that forces Phase A path.

**C3. Depth tiers (plan §7)**

| Tier | Behavior | Use |
|------|----------|-----|
| `quick` | contract_tape only, cached mids, no yfinance | Board scan |
| `standard` | technical + contract_tape + news if available | Default Analyze/chat |
| `deep` | + macro + fresh history | Manual deep button |

Implement as `context.depth` field on `MarketOpinionRequest` (schema already has free-form `context`).

**C4. Settings**

- Show last agent call latency, last error, opining rate (opining / total calls) from bridge counters.

**Acceptance C**

- User can see agent contribution without reading SQLite.
- Batch scan does not block UI (async + progress).

---

### Phase D — Hygiene and true Fincept depth (later)

1. **AGPL isolation:** split `fincept-sidecar` to public repo before large Fincept-derived ports (plan §3 Rule 1).
2. Port Fincept modules only where data path is real (macro EconDB, fundamentals for company events).
3. Stocks/crypto continuous `AssetSignal` (plan §14) after binary calibration gate matures.

---

## What not to do

- Do **not** embed Python in the Tauri process (AGPL + crash domain).
- Do **not** fabricate agent probabilities to “fill” the board.
- Do **not** change Kelly / shrink / fee math under the guise of integration.
- Do **not** dump full agent JSON for 80 markets into every chat prompt (latency + monologue regression). Cap to 3 markets, compact lines.
- Do **not** block chat first-token on sidecar (parallel + timeout; degrade gracefully).

---

## Suggested execution order (tracer bullets)

| Sprint | Deliverable |
|--------|-------------|
| **1** | A1–A4: shared input builder, mids always, chat agent priors, unified ledger |
| **2** | B1 + B2: technical coverage + news agent from web snippets |
| **3** | C1–C3: Edge Board + depth tiers + UI drawer |
| **4** | B3 macro (FRED) when economic markets are a focus |
| **5** | D: repo split + optional Fincept module extract |

---

## Verification

### Automated

- Rust: `cargo test --lib edge_engine:: pipeline fincept_bridge chat::fincept`
- Sidecar: `uv run pytest` in `fincept-sidecar` (technical math, schemas, auth)
- Integration smoke: `scripts/live_forecast_pipeline.py` with bridge running → non-null `p_model` on price-level tickers
- Vitest: Calibration / Command desk agent breakdown rendering

### Manual E2E

1. Start app → Settings Fincept shows online.
2. Command desk: pick BTC/index market → Analyze → agent drawer shows technical and/or contract_tape.
3. Analyst: “mispriced today” → system context (or UI chip) shows SIDECAR MODEL PRIORS; JSON decision can cite them.
4. Kill sidecar → chat still answers; Analyze writes market-only ledger row with `sidecar_unavailable` reason.
5. SQLite: `SELECT market_ticker, p_model, agent_breakdown FROM forecasts ORDER BY id DESC LIMIT 10` — agent fields populated when online.

### Success metrics (process, not fabricated PnL)

| Metric | Baseline (current) | Target after Sprint 1–2 |
|--------|--------------------|-------------------------|
| Forecasts with any agent `probability != null` | ~few / mostly null | Majority of price-level + high-mid markets with history |
| Chat sessions that receive agent priors | ~0% | ~100% when bridge online + open markets |
| Macro/news opining rate | 0% | News >0% on politics/econ with web; macro >0% when FRED mapped |
| Analyst reliance on invented fairs | High | Lower: must reconcile to sidecar prior or explain override |

---

## Bottom line

The sidecar is not missing as a **process** — it is missing as a **default opinion source**. Highest leverage: **call `market-opinion` (via the edge pipeline) from Analyst for a few open markets, always feed mids + underlying, and grow honest agents (news/macro/technical coverage)** before more Fincept code lands. That turns Fincept from “Settings online + World Markets tab” into the co-pilot that actually shapes `p_model` and chat fair values.
