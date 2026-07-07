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
