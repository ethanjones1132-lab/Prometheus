# Kalshi Monster × Fincept Terminal — Integration Groundwork

Date: 2026-07-07 (scheduled run, `fable-kalshi-monster`)
Status: **Groundwork / pre-implementation** — source plan not yet accessible (see Provenance)
Author: Automated pass (Claude), grounded in repo code and live upstream research

---

## Provenance and a blocker to resolve first

The scheduled task points at `C:\Users\ethan\OneDrive\Desktop\kalshi-monster-fincept-integration-plan.md`. That path is **outside the connected `kalshi-build` folder**, so scheduled (non-interactive) runs cannot read it, and no copy or `fincept` reference exists anywhere in the repo (verified by full-text search).

Rather than fabricate an implementation of a plan I could not read, this pass did the highest-value work that any version of that plan will need: a verified survey of both codebases' integration surfaces, a licensing/risk analysis that likely **changes the plan's viability assumptions**, and a recommended architecture with concrete interface sketches.

**Action for Ethan:** copy the plan into the repo (suggested: `kalshi-monster/docs/fincept-integration-plan.md`). The next scheduled run will pick it up and execute against it directly.

---

## Finding 1 — Fincept Terminal v4 is no longer a Tauri/Rust app

This is the single most important fact checked this run, because it likely invalidates any integration plan drafted against older Fincept documentation.

Verified from the upstream repo (Fincept-Corporation/FinceptTerminal, v4.0.2, released April 2026):

| Property | Fincept Terminal v4 | Kalshi Monster v0.8.0 |
|---|---|---|
| UI/runtime | **Native C++20 + Qt6** (complete rewrite; previous Tauri 2 + React + Rust stack abandoned) | Tauri 2 + React/TypeScript |
| Analytics layer | Embedded Python 3.11 (QuantLib suite, ML, agents) | Rust + Python sidecar (`ml_predictor.py`) |
| Data layer | 100+ connectors (DBnomics, Polygon, Kraken, Yahoo, FRED, IMF, World Bank, brokers…) | Kalshi public API client (Rust) |
| Extensibility | Visual node editor with **MCP tool integration**; Python data connectors | Tauri commands (~80 handlers in `commands/mod.rs`) |
| Execution | Real-time trading, 16 broker integrations | **Analytics/paper only — never places orders** (hard product guarantee) |
| License | **AGPL-3.0 + Fincept Commercial License (dual)** | MIT |

Consequences:

- There is **no shared Rust/Tauri substrate** to link against. Any plan step that assumed importing Fincept crates, sharing Tauri plugins, or merging frontends is dead on arrival.
- The realistic integration boundaries are **process-level**: MCP, Python connector, HTTP/IPC, or file/DB interchange — not code-level embedding.

## Finding 2 — Licensing is the controlling constraint

Fincept's terms are unusually aggressive and must shape the architecture:

- Dual license: AGPL-3.0 for personal/academic use; a **paid commercial license for any business use**, explicitly including forks, internal tools, and consulting deliverables. Their README claims liquidated damages starting at USD 50,000/org/year and asserts the obligation survives even if Fincept APIs are stripped from a fork.
- Kalshi Monster is MIT-licensed. **Vendoring or deriving from Fincept code would contaminate the repo** (AGPL copyleft at minimum; Fincept's commercial-license claims beyond that).
- Arm's-length interoperability (Kalshi Monster and Fincept running as separate programs exchanging data over MCP/HTTP/files) does **not** create a derivative work and is the only posture that keeps kalshi-monster MIT-clean.
- Contributing a *Kalshi connector* upstream to Fincept's repo is fine for their ecosystem but licenses that contribution under their terms — keep any such connector code out of this repo, or dual-write it from a clean-room spec.

**Recommendation: no Fincept source code, snippets, or vendored modules in `kalshi-build` — integrate at process boundaries only.**

## Finding 3 — Kalshi Monster's integration surface is already well-factored

Survey of `src-tauri/src` (verified against code this run):

- `kalshi/` — `KalshiClient` (fetch/search markets, orderbook, balance, positions, category stats), `SharedCache: Arc<RwLock<Option<KalshiCache>>>`, persisted market-cache store, price tracker (`get_price_history`, `snapshot_markets`), grading (`evaluate_bet`, auto-grade task), portfolio risk (`compute_stake_adjustment`, exposure builders).
- `predictions/` + `predictions.db` (SQLite, canonical at `~/.openclaw/kalshi-monster/`) — paper journal, stats, calibration; the graded history that Phase 3 ML is waiting on.
- Shared edge math in sibling crates `monster-edge-core` / `edge-eval` (MIT-side, reusable freely).
- ~80 Tauri commands in `commands/mod.rs` already expose config, markets, predictions, bankroll, recommendations, and grading as clean request/response functions — an almost ready-made tool catalog.

The important structural point: **everything Fincept would want from Kalshi Monster already exists behind a command layer.** Integration is an exposure problem, not a rebuild problem.

## Recommended architecture — MCP server boundary (Option A)

Fincept v4's node editor natively consumes **MCP tools**. Kalshi Monster can ship a small MCP server (stdio transport, Rust, reusing the existing modules — not the Tauri runtime) that exposes read-only research tools. Fincept then becomes *one of several possible clients* (Claude/Cowork could consume the same server), and no Fincept code ever touches this repo.

Proposed initial tool surface (each maps ~1:1 onto existing functions):

| MCP tool | Backing implementation |
|---|---|
| `kalshi_search_markets(query)` | `KalshiClient::search_markets` |
| `kalshi_market_detail(ticker)` | `fetch_market` + `fetch_orderbook` |
| `kalshi_top_markets(limit, category?)` | `get_top_markets` / `get_markets_by_category` |
| `kalshi_price_history(ticker)` | `price_tracker::get_price_history` |
| `paper_journal_stats()` | prediction stats + calibration metrics |
| `paper_positions_risk()` | `exposures_from_predictions` + `compute_stake_adjustment` |
| `edge_assessment(ticker, fair_prob)` | `monster-edge-core` EV/Kelly + calibrator |

Guardrails to preserve the product guarantee: read-only tools only; no tool that writes the journal, alters config, or hints at order placement; balance/positions tools excluded by default (opt-in config flag) since they expose account data to an external client.

Placement suggestion: new sibling crate `kalshi-build/kalshi-mcp/` depending on a factored-out core (or, more cheaply, a `--mcp` headless mode inside `src-tauri` gated behind a feature flag). The sibling-crate route keeps the Tauri app untouched and testable in isolation.

### Alternatives considered

- **Option B — Kalshi data connector contributed upstream to Fincept (Python):** gives Fincept users Kalshi market data, but does nothing for kalshi-monster's own analytics, and the code would live under Fincept's license. Complementary, not a substitute; do it second if at all.
- **Option C — File/DB interchange (CSV/Parquet exports of journal + snapshots):** trivial to build (`export_predictions_csv` already exists), zero licensing risk, but batch-only and no live workflows. Reasonable fallback; the enterprise plan already lists journal export as a P-item.
- **Option D — Code-level embedding of Fincept analytics:** rejected. Wrong ABI (C++/Qt), AGPL/commercial contamination, and duplicates analytics kalshi-monster already gets from `edge-eval`.

## Suggested sequencing (pending the real plan)

1. Ethan copies the actual plan into `kalshi-monster/docs/` — reconcile it against this document, especially any step assuming Fincept's old Tauri stack.
2. Decide the licensing posture explicitly (personal-use AGPL vs. any commercial ambition) before any Fincept-side work.
3. Build the MCP server skeleton (Option A) with the three cheapest tools (`kalshi_search_markets`, `kalshi_market_detail`, `paper_journal_stats`) and integration-test from any MCP client.
4. Wire into Fincept's node editor as an MCP tool source; validate a research workflow end-to-end (e.g., Fincept macro dashboard → Kalshi CPI market probabilities side-by-side).
5. Only then consider Option B/C extensions.

Nothing was committed to git this run; this document is the only change (untracked).

---

Sources: [FinceptTerminal repo (v4 README, license, architecture)](https://github.com/Fincept-Corporation/FinceptTerminal) · repo code under `kalshi-build/kalshi-monster` (README, PRIORITIES, ROADMAP, `src-tauri/src/kalshi/*`, `commands/mod.rs`, `docs/enterprise-buildout-plan.md`)
