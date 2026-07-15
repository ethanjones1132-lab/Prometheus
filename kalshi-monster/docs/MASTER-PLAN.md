# Kalshi Monster — Master Implementation Plan

**Status:** Active roadmap (forward-looking)  
**Created:** 2026-07-15  
**Merges:**

| Source plan | Path |
|-------------|------|
| Fincept sidecar deep integration | `docs/fincept-sidecar-deep-integration-plan.md` |
| Paper system audit | `docs/paper-system-audit-2026-07-15.md` |
| Model session performance (process quality) | `reports/model-session-performance-2026-07-14.md` |
| Fleet / Fincept product backlog (context) | `docs/fincept-integration-progress.md`, `docs/fincept-integration-plan.md` v2.1 |

**This file is the source of truth for “what to build next.”** Older plans remain historical; do not re-open completed phases unless regressing.

---

## Hard constraints (all workstreams)

1. **AGPL boundary:** Fincept-derived logic stays in the sidecar process (HTTP only). No Python embedding in Tauri.
2. **No invented probabilities:** agents return `probability=None` when data is missing.
3. **No Kelly / EV / shrink / fee formula rewrites** unless a dedicated math ADR says otherwise. Wiring, rails, agents, and UX only unless explicitly scoped.
4. **Calibration honesty:** no fabricated forecast outcomes; gate live trading on real resolved data.
5. **Paper cash integrity:** settlement must be side-aware; lots only on TAKE; predictions stay in sync when lots close.

---

## Already shipped (do not re-implement)

### Model / Analyst process quality
- Bid/ask/mid context labels; retrieval prefers tight liquid books  
- JSON-first prompts; deliverable strip; resend mode; quality rails  
- Track-record card; free-tier model note; stream thoughts separated  
- Decision extract: last-valid JSON, placeholder reject, unit coerce  

### Fincept Phase A
- `edge_engine/opinion_input` (mids + underlying/strike)  
- Chat injects `## SIDECAR MODEL PRIORS` (up to 3 open markets)  
- Analyze uses shared builder; chat ledger prefers edge pipeline + LLM annotation  

### Paper correctness + settle sync
- NO-side settlement fix; TAKE-only lots; entry dollars  
- IPC serde hardening (`model_disagreement`, `ChatExtract`, risk `Other`, …)  
- MarketDetailPanel risk flag codes; truthful chat paper messaging  
- `prediction_id` on lots; settle syncs prediction Win/Loss; grade/resolve/auto-grade settle paper  

### Sprint 0 — Paper journal trust (done 2026-07-15)
- Structured `PaperRecordResult` IPC (`lot_opened`, `demotion_notes`, …); Chat/MarketDetail show journal vs lot  
- Forecast `close_time` from market tape (close/expiry), not wall-clock  
- Equity snapshots: open MV falls back to cost basis (no cash-only crash); `profit_factor` capped  
- Breakers: refuse **new paper lots** on daily-loss pause / hard disable; `paper_only` demotion still allows lots  

### Sprint 1+2 — Agents + Edge Board (done 2026-07-15)
- Technical: expanded series map (`KXBTCD`/`KXETHD`/index/majors), barrier strike from ticker, `horizon_days`  
- News agent: `agents/news.py` over Rust `web_snippets` (null unless grounded); orchestrator wired  
- Silent routing weight in `verdict_reasons` when macro/news hold mass but null  
- Edge Board on Calibration: scan top-N ranked by `|edge_net|`, agent drawer, Deep top 3  
- Analyst chip: Sidecar online + Deep analyze top 3  

### Sprint 3 — Depth tiers + sidecar ops (done 2026-07-15)
- `AnalysisDepth` + `context.depth`: board=`quick`, Analyze=`standard`, Deep top 3=`deep`  
- Sidecar orchestrator skips technical/news on quick; Settings shows latency / opining rate  
- Release packaging: `tauri.conf.release.json` externalBin + cargo test guard  

---

## Master sequence (chronological from now)

Work is ordered so each step **unblocks trust or p_model quality** before bigger product surface.  
Estimate bands are rough (1 = short pass, 3 = multi-day).

| # | Sprint | Theme | Effort | Depends on | Source |
|---|--------|--------|--------|------------|--------|
| **0** | **P0 — Trust polish (paper journal UX)** ✅ | Structured paper IPC + real close_time + equity MTM | 1–2 | — | Paper audit P1–P2 |
| **1** | **P1 — Agent opinions that fire** ✅ | Technical coverage + news agent | 2 | Phase A done | Fincept B1–B2 |
| **2** | **P2 — Edge Board v1** ✅ | Rank markets by edge; agent drawer | 2 | P1 helps | Fincept C1–C2 |
| **3** | **P3 — Depth tiers + Settings ops** ✅ | quick/standard/deep; bridge metrics | 1–2 | P1 | Fincept C3–C4 |
| **4** | **P4 — Macro agent (economic)** | FRED/public series, honest nulls | 2 | P1 | Fincept B3 |
| **5** | **P5 — Calibration flywheel** | Accumulate resolved ≥200; λ re-fit ops | ongoing | settle/grade live | Progress / Phase 3 |
| **6** | **P6 — Paper product polish** | Bankroll vs cash clarity; Grade vs Settle UX | 1–2 | P0 | Paper audit M/L |
| **7** | **P7 — AGPL + deep Fincept** | Sidecar public repo; optional module ports | 3+ | P1–P4 stable | Fincept D |

---

## Sprint 0 — Paper journal trust ✅ (done 2026-07-15)

**Why first:** Auto-settle/sync is in, but breakers and UX still misread paper health. Finish paper integrity before leaning harder on agent-driven TAKEs.

| ID | Task | Status |
|----|------|--------|
| **0.1** | Structured `kalshi_record_paper_decision` → `PaperRecordResult` | ✅ Chat/MarketDetail show lot vs journal |
| **0.2** | Paper forecast `close_time` from tape | ✅ close/expiry from cached/fetched market |
| **0.3** | Equity snapshot open MV (client or cost-basis) | ✅ + `equity_snapshot_uses_cost_basis_when_no_marks` |
| **0.4** | Refuse new lots on daily pause / hard disable | ✅ `paper_only` still allows lots (§6.4) |

**Out of scope here:** fee formula changes, live orders.

**Verified:** `cargo test --lib paper::` **11/11**; MarketDetailPanel vitest **4/4**; `tsc --noEmit` clean.

---

## Sprint 1 — Fincept Phase B ✅ (done 2026-07-15)

**Why next:** Phase A wires the pipe; most categories still get `p_model=null`. Fill honest signal before Edge Board ranking.

| ID | Task | Status |
|----|------|--------|
| **1.1** | Expand technical map + horizon_days | ✅ Rust + sidecar series/barrier/horizon |
| **1.2** | News agent + web_snippets | ✅ `agents/news.py`; deep/single analyze attaches snippets |
| **1.3** | Silent agent weight in verdict_reasons | ✅ `silent_agent_weight_report` |
| **1.4** | Sidecar pytest technical + news null | ✅ `tests/test_news_and_technical_null.py` |

---

## Sprint 2 — Edge Board v1 ✅ (done 2026-07-15)

| ID | Task | Status |
|----|------|--------|
| **2.1** | Batch analyze top-N; rank by \|edge_net\| | ✅ `rank_by_abs_edge_net` + Calibration Edge Board |
| **2.2** | Agent breakdown drawer | ✅ Click row → signals/rationale/reasons |
| **2.3** | Analyst sidecar chip | ✅ ChatView status pill |
| **2.4** | Deep analyze top 3 | ✅ Calibration + Analyst buttons (`deep=true`) |

**Verified:** `cargo test --lib edge_engine::` 66; Calibration/Chat vitest; sidecar news/technical pytest.

---

## Sprint 3 — Depth tiers + sidecar ops UX ✅ (done 2026-07-15)

| ID | Task | Status |
|----|------|--------|
| **3.1** | `context.depth` quick / standard / deep | ✅ Board quick; Analyze standard; Deep top 3 deep |
| **3.2** | Settings bridge ops counters | ✅ latency, opining rate, call totals |
| **3.3** | Release `externalBin` for fincept-sidecar | ✅ release conf + unit test |

**Verified:** fincept_bridge + pipeline depth tests; Settings vitest; sidecar quick-depth pytest.

---

## Sprint 4 — Macro agent (economic contracts)

| ID | Task | Acceptance |
|----|------|------------|
| **4.1** | Map CPI/Fed/payrolls-style tickers → public series (FRED key optional) | Mapped econ contracts can opine |
| **4.2** | Unmapped → `probability=None` + clear rationale | No fake macro edge |
| **4.3** | Routing honesty when macro silent | Verdict reasons mention weight on null |

**Defer:** full Fincept EconDB extract until Sprint 7 AGPL hygiene.

---

## Sprint 5 — Calibration flywheel (ongoing, parallel after 0–1)

Not a single PR — operational + light product.

| ID | Task | Acceptance |
|----|------|------------|
| **5.1** | Keep auto-grade + paper settle running on user machines | Resolved rows accumulate |
| **5.2** | Gate dashboard: resolved count, Brier model vs market, paper equity | Clear LOCKED/OPEN state |
| **5.3** | λ re-fit when n sufficient; persist edge_config | Already partly shipped; polish UX |
| **5.4** | Prefer stronger model for live TAKE (settings guidance only) | Doc + Settings copy |

**Target:** ≥200 resolved forecasts before treating paper/live edge as validated.

---

## Sprint 6 — Paper product polish

| ID | Task | Acceptance |
|----|------|------------|
| **6.1** | Size / display paper **cash** vs bankroll.json clearly | No dual-ledger confusion |
| **6.2** | Portfolio: Grade vs Settle copy; auto-refresh after chat record | Users understand two actions |
| **6.3** | Chat bankroll policy into extractPaperDecision | Client stake ≈ server |
| **6.4** | Profit factor finite (no Infinity JSON); analytics error surfaces | Panel never blank-fails |
| **6.5** | Optional: fee preview on TAKE; confirm large stake | Safer UX |

---

## Sprint 7 — AGPL isolation + deep Fincept (last)

| ID | Task | Acceptance |
|----|------|------------|
| **7.1** | Split `fincept-sidecar` to public repo before large Fincept-derived ports | Plan §3 Rule 1 |
| **7.2** | Port only modules with real data paths (macro DB, fundamentals) | Honest agents only |
| **7.3** | Stocks/crypto continuous `AssetSignal` after binary calibration matures | Plan §14 |

---

## Dependency graph (summary)

```text
[0 Paper trust polish]
        │
        ▼
[1 Agents that fire: technical + news] ─────┬──► [4 Macro]
        │                                   │
        ▼                                   │
[2 Edge Board v1] ◄─────────────────────────┘
        │
        ▼
[3 Depth tiers + Settings ops]
        │
        ├── parallel ──► [5 Calibration flywheel]
        │
        ▼
[6 Paper product polish]
        │
        ▼
[7 AGPL split + deep Fincept]
```

---

## What not to do (carry-forward)

- Do not invent agent probabilities to fill the board.  
- Do not change Kelly/EV/shrink math without an ADR.  
- Do not dump 80 markets of agent JSON into every chat turn.  
- Do not block first chat token on sidecar (timeout + degrade).  
- Do not open paper lots on WATCH/PASS.  
- Do not settle paper with YES-only exit prices.  
- Do not treat market entry-price backtest as LLM skill.  

---

## Definition of done (program-level)

The program is “ready for serious paper validation” when:

1. Analyst always sees sidecar priors when bridge is online (Phase A).  
2. ≥1 non-null agent regularly fires on price-level markets (Sprint 1).  
3. Edge Board ranks markets with agent attribution (Sprint 2).  
4. Paper open → MTM equity sensible; resolve → lot + prediction + forecast agree (Sprint 0 + settle sync).  
5. Resolved forecast n growing toward calibration gate (Sprint 5).  

---

## Pointers for agents

| If you are working on… | Read first |
|------------------------|------------|
| Next feature pick | **This file** (`docs/MASTER-PLAN.md`) |
| Fincept agent details | `docs/fincept-sidecar-deep-integration-plan.md` |
| Paper engine details | `docs/paper-system-audit-2026-07-15.md` |
| Analyst quality history | `reports/model-session-performance-2026-07-14.md` |
| Long-range architecture | `docs/fincept-integration-plan.md` |
| Chronology / ship log | `PRIORITIES.md` (notes only; not the order source) |

**Default next implementation:** **Sprint 4** (macro agent), then Sprint 5/6 polish.
