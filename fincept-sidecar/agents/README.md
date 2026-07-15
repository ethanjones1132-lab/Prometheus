# agents/ — the AGPL heart (plan §3 Rule 1, §5)

This package is where Fincept-derived agent code lands: the 9-agent framework
(Macro, Technical, Valuation, Fundamentals, Sentiment, News, Risk Manager,
Portfolio Manager, Explainability) with the prediction-market adaptations
from plan §5.1.

Rules for anything added here:

1. It ships under AGPL-3.0 with upstream attribution in NOTICE.
2. It must emit the `AgentSignal` contract from `fincept_sidecar/schemas.py` —
   `probability=None` when the agent has no relevant data, never a
   hallucinated number.
3. Every data input carries a `DataRef` with its fetch timestamp (staleness
   enforcement, plan §4.5).
4. Nothing in here touches money: no Kalshi credentials, no order placement,
   no bankroll state. Agents produce opinions; the Rust core decides.
5. Before the first Fincept file lands: split this repo out of `kalshi-build`
   into its own public repository (plan §3 Rule 1) — see README.

Phase 2 extraction spike (plan §13.2, timeboxed 2 days): determine whether
the v4 embedded-Python modules lift cleanly, or fall back to the v1.x
pure-Python tree.

## Implemented now (2026-07-15)

| Agent | Module | Data source | Notes |
|-------|--------|-------------|-------|
| **technical** | `technical.py` | yfinance via `engines/market_data.py` | Lognormal binary `P(S_T>K)` from realized vol + optional micro-momentum μ. Series prefixes (`KXBTCD`…), barrier strike, `horizon_days`. Opines only when underlying+strike+horizon are known. |
| **contract_tape** | `contract_tape.py` | `context.contract_mids` from Rust (Kalshi) | Momentum + mild longshot-bias adjustment on the contract's own mid path. `probability=None` without a real series. |
| **news** | `news.py` | `context.web_snippets` from Rust (Brave/Tavily/DDG) | Heuristic lean over grounded snippets only. `probability=None` without snippets or when language is inconclusive — never invents p. |
| **macro** | `macro.py` | FRED public API (`FRED_API_KEY` optional) | Maps CPI/Fed/payrolls/GDP/unemployment tickers → series; opines only with threshold + data. Null when unmapped or key missing. |

### Explicitly not implemented (no honest data path yet)

- **sentiment** — need social/news sentiment feeds
- **valuation / fundamentals** — need fundamentals DB
- Full Fincept EconDB extract (deferred to Sprint 7 AGPL hygiene)
- Risk / portfolio / explainability shape sizing & reporting in Rust, not `p_model`

Orchestrator: `orchestrator.py` → `POST /api/v1/agents/market-opinion`.
