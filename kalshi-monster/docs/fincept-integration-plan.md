# Kalshi Monster × Fincept Terminal Integration Plan

**Document Version:** 2.1
**Date:** July 6, 2026
**Author:** Claude (commissioned by Ethan Jones)
**Status:** Architectural Plan — Ready for Review
**Supersedes:** v1.0 and v2.0 (see Appendix D for what changed and why; v2.1 adds Section 14: the stocks-and-crypto expansion roadmap)

---

## Executive Summary

This plan integrates Fincept Terminal's Python analysis engine into Kalshi Monster (currently branded PrizePicks Monster v0.8.0) to turn it from a data-starved demo into a working prediction-market decision engine.

The single most important reframing from v1.0: **the near-term goal is not to surface Fincept's 16 data sources inside Kalshi Monster — it is to answer one question well: "Is this instrument mispriced, by how much, and how much should I stake?"** Every integration decision below is judged against that question. Data sources that don't sharpen it (YouTube sentiment, Google Scholar research, India open data, the Fincept community forum) are explicitly moved to a backlog rather than given implementation phases.

The **long-term destination — and the real reason for the Fincept integration — is a multi-asset decision engine spanning prediction markets, stocks, and crypto.** Prediction markets are the beachhead, not the whole territory: they are where the pipeline (forecast → prior blend → cost hurdle → fractional Kelly → calibration gate) gets proven with the smallest, most measurable stakes, because binary contracts resolve fast and score cleanly. Section 14 scopes the expansion into stocks and crypto — including exactly which pieces of math change shape when payoffs go from binary to continuous, because getting that math right is the difference between a portfolio engine and a leak. The architecture below is designed so that expansion is a new asset book on existing rails, not a second system.

The architecture remains a **Python sidecar behind a local HTTP API**, but v2.0 justifies that choice against real alternatives (PyO3 embedding, stdio JSON-RPC, a hosted service, a Rust rewrite) and adds the argument v1.0 missed entirely: Fincept Terminal is licensed **AGPL-3.0**, and the sidecar's process boundary is also the *license* boundary that keeps Kalshi Monster's Rust/React codebase from becoming a derivative work. Section 3 covers the compliance strategy in detail.

Three other structural changes from v1.0:

1. **A new Phase 0 fixes Kalshi Monster's existing dead code before any Fincept work begins.** The prop board runs on hardcoded demo data, `MarketContextBuilder` is never called, and `ml.rs` is dead code. Wiring up what already exists is the highest-ROI work in the entire plan and requires zero new infrastructure.
2. **A calibration gate stands between analysis and real money.** The system paper-trades and logs every forecast until the model demonstrably beats the market price at predicting outcomes (measured by Brier score over ≥200 resolved forecasts). No live order flow ships before that gate is passed.
3. **The edge engine gets rigorous math.** Kalshi charges trading fees of roughly `0.07 × P × (1−P)` per contract; markets have spreads; the market price is itself a strong probability estimate that the model must be shrunk toward. v1.0's "market says 72%, we say 84%, bet the difference" logic loses money in practice. Section 4 replaces it.

The honest caveat that v1.0 omitted: **Fincept does nothing for the sports prop side of the app.** It has no player stats, no injury data, no sportsbook lines. This plan makes Kalshi (event/economic/financial markets) the primary domain and flags the sports prop data gap as a separate problem needing a separate solution (Section 13).

---

## Table of Contents

1. [What Each System Actually Brings](#1-what-each-system-actually-brings)
2. [Architecture Decision](#2-architecture-decision)
3. [AGPL-3.0 Licensing Strategy](#3-agpl-30-licensing-strategy)
4. [The Edge Engine: Model Probability vs. Market Price](#4-the-edge-engine-model-probability-vs-market-price)
5. [Mapping the 9 AI Hedge Fund Agents to Prediction Markets](#5-mapping-the-9-ai-hedge-fund-agents-to-prediction-markets)
6. [Risk Management and Position Sizing](#6-risk-management-and-position-sizing)
7. [Phased Implementation](#7-phased-implementation)
8. [Data Flow Architecture](#8-data-flow-architecture)
9. [Frontend Plan](#9-frontend-plan)
10. [Technical Details: Lifecycle, Caching, Security, Config](#10-technical-details)
11. [Testing and Calibration Discipline](#11-testing-and-calibration-discipline)
12. [Risks and Mitigations](#12-risks-and-mitigations)
13. [Known Gaps and Open Questions](#13-known-gaps-and-open-questions)
14. [Future Work: Extending to Stocks and Crypto](#14-future-work-extending-to-stocks-and-crypto)
- [Appendix A: Endpoint Reference](#appendix-a-endpoint-reference)
- [Appendix B: Data Source Triage](#appendix-b-data-source-triage)
- [Appendix C: Kalshi Fee and Kelly Reference Math](#appendix-c-kalshi-fee-and-kelly-reference-math)
- [Appendix D: Changes from v1.0](#appendix-d-changes-from-v10)

---

## 1. What Each System Actually Brings

### Kalshi Monster (PrizePicks Monster v0.8.0)

**Architecture:** Tauri 2 desktop app — Rust backend, React/TypeScript frontend, SQLite persistence.

**Working today:**

- Desktop shell with tabbed navigation (Chat, Kalshi Markets, Props, Predictions, Settings)
- Kalshi API client: JWT auth, market search, category browsing, in-memory market index
- Prop scoring engine: edge %, EV %, risk classification, recommendation tiers
- Kelly criterion bankroll manager with a 10% hard cap
- OpenRouter LLM chat with streaming
- Discord + Telegram notification pipeline
- CSV prediction export; persistent config and prediction history in SQLite

**Broken or missing (in priority order):**

| Gap | Severity | Fixed by |
|---|---|---|
| Prop board is 5 hardcoded demo entries | Blocks everything | Phase 0 (Kalshi data) + separate sports-data decision |
| `MarketContextBuilder::build_context_packet()` exists but is never called | AI chat reasons blind | Phase 0 |
| `ml.rs` logistic regression is dead code | Wasted surface area | Phase 0 (delete or revive — decide, don't leave) |
| No forecast ledger — recommendations vanish without outcome tracking | Can't measure whether the system works | Phase 0 |
| No order placement (`place_order()` missing; types exist) | Can't act on edge | Phase 5, gated on calibration |
| No data beyond Kalshi: no equities, macro, news, or cross-asset context | Model probabilities have nothing to stand on | Phases 1–2 |
| Kelly sizing is single-position; no correlation or exposure awareness | Overbets correlated events | Phase 4 |

### Fincept Terminal

**Version correction (important).** v1.0 of this plan described Fincept Terminal v1.0.7, a pure-Python TUI built on Textual. The current Fincept Terminal v4 is a **C++20/Qt6 desktop application with an embedded Python runtime** — the C++ layer is the shell and UI; the analysis logic still lives in Python modules executed by the embedded interpreter.

This changes *what we take*, not *whether we take it*: the integration target is **Fincept's Python analysis layer** (the embedded modules in v4, cross-referenced against the fully-Python v1.x codebase where the v4 embedding obscures module boundaries). The C++/Qt6 shell is irrelevant to us — Kalshi Monster already has a shell. We are extracting the engine, not the car.

**What we extract, by value to the edge engine:**

| Tier | Capability | Why it matters for Kalshi decisions |
|---|---|---|
| **Core** | 9-agent AI hedge fund framework (Sentiment, Fundamentals, Valuation, Technical, Risk, Portfolio Manager, Macro, News, Explainability) | The probability-estimation machinery. Section 5 maps each agent to prediction-market work. |
| **Core** | EconDB macro indicators (GDP, CPI, unemployment, yield curves, money supply, policy rates for 100+ countries) | Kalshi's economic contracts (CPI, Fed, GDP, jobs) resolve *directly against these numbers*. This is the closest thing to ground truth the system will have. |
| **Core** | Live market data: 132 instruments across 6 asset classes via yfinance; OHLCV history | Underlying price series for index/price-level contracts; volatility inputs; cross-asset context |
| **High** | Technical analysis (MA, RSI, MACD) and quant metrics (Sharpe, vol, drawdown) | Distributional forecasts for "will S&P close above X" contracts |
| **High** | Portfolio analytics (empyrical: returns, Sharpe, Calmar, Sortino, alpha/beta, drawdown) | Portfolio-level risk in Phase 4 |
| **High** | News integration (GNews, RSS) | Event catalysts and resolution-relevant facts |
| **Medium** | Multi-LLM support (Gemini, OpenAI, Ollama) | Provider redundancy and local/private inference |
| **Medium** | Robo advisor screening; comparison engines | Nice-to-have research tools; not edge-critical |
| **Backlog** | YouTube sentiment (RoBERTa + transcripts), Google Scholar research, DataGovIN India data, Fyers broker, community forum | See Appendix B for triage rationale |

**What Fincept does *not* bring (be honest about this):**

- No sports data of any kind — no player stats, injuries, lines, or historical box scores
- No prediction-market data — no Kalshi, Polymarket, or Manifold integration
- No order execution relevant to Kalshi (Fyers is Indian equities)
- Agents were built for equities; every agent needs a prediction-market adaptation layer (Section 5)

---

## 2. Architecture Decision

### 2.1 Options Considered

**Option A — Python sidecar behind local HTTP (FastAPI). Recommended.**
Fincept's Python modules run as a separate process; Kalshi Monster's Rust backend calls `http://127.0.0.1:<port>` via `reqwest` (already a dependency). Tauri's `tauri-plugin-shell` (already in `Cargo.toml`) manages the process lifecycle.

- ✅ **License boundary.** The AGPL-3.0 sidecar and the Kalshi Monster app remain separate programs communicating at arm's length. This is the decisive argument — see Section 3.
- ✅ No rewrite of either codebase; Python keeps its ecosystem (yfinance, pandas, scipy, transformers).
- ✅ Concurrent long-running requests (agent runs take 10–30 s) are natural over HTTP; FastAPI gives typed schemas and OpenAPI docs for free.
- ✅ Can later be swapped for a remote server without touching Rust call sites.
- ⚠️ Packaging: a PyInstaller bundle with torch/transformers is ~2 GB. Mitigated by making ML-heavy sentiment optional (Section 10.4).
- ⚠️ Port management, process supervision, and localhost security need explicit handling (Section 10.1–10.2).

**Option B — Embed Python in Rust via PyO3.** Rejected. Two independent dealbreakers: (1) linking AGPL Python code into the Kalshi Monster process almost certainly creates a combined work, forcing the entire app under AGPL-3.0; (2) the GIL serializes the analysis workload, and shipping an embedded CPython with torch inside a Tauri bundle is strictly harder than shipping a sidecar exe.

**Option C — JSON-RPC over stdio.** Considered seriously — it eliminates port management and any network surface. Rejected because: agent runs are long and concurrent (stdio framing makes multiplexing and cancellation painful); no free OpenAPI/schema tooling; streaming partial results (progressive agent output to the UI) is much cleaner over HTTP/SSE. If the localhost port ever becomes a real problem, this is the fallback, and the `FinceptBridge` abstraction in Rust is designed so the transport can be swapped.

**Option D — Rewrite the analysis layer in Rust.** Cleanest licensing (no AGPL code at all) and best runtime, but months of work re-implementing yfinance-equivalents, empyrical, EconDB clients, and agent logic before any value ships. Rejected for now; noted as the long-term escape hatch if AGPL obligations ever become commercially untenable.

**Option E — Hosted Fincept service (remote API).** Rejected for v1: it triggers AGPL §13's network clause obligations anyway, adds hosting cost and latency, and puts the user's API keys on a server. The sidecar design deliberately keeps the door open (the Rust side only knows a base URL).

### 2.2 Selected Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                     TAURI DESKTOP SHELL                          │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  React/TS Frontend: Chat │ Markets │ Edge Board │ Portfolio │ │
│  └──────────────────────────┬─────────────────────────────────┘ │
│                     Tauri commands                               │
│  ┌──────────────────────────┴─────────────────────────────────┐ │
│  │  RUST BACKEND                                               │ │
│  │   KalshiClient ── EdgeEngine ── BankrollManager ── SQLite   │ │
│  │        │              │              │                      │ │
│  │   ForecastLedger  SignalAggregator  AlertEngine             │ │
│  │        └──────────────┴──────┬───────┘                      │ │
│  │              FinceptBridge (reqwest, bearer token)           │ │
│  └──────────────────────────────┬──────────────────────────────┘ │
│              HTTP, 127.0.0.1:<ephemeral port>                    │
│  ┌──────────────────────────────┴──────────────────────────────┐ │
│  │  FINCEPT SIDECAR (Python/FastAPI, separate process,          │ │
│  │  separate repo, AGPL-3.0)                                    │ │
│  │   /market/*   /agents/*   /economic/*   /portfolio/*         │ │
│  │   MarketDataEngine · 9 Agents · EconDB · Analytics            │ │
│  └───────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

Division of responsibility, stated once and enforced everywhere:

- **Python sidecar** owns *analysis*: data fetching from external sources, agent runs, probability estimates, analytics. It is stateless apart from its caches; it holds no bankroll state and never talks to Kalshi's trading endpoints.
- **Rust backend** owns *decisions and money*: edge computation, shrinkage, Kelly sizing, exposure limits, order placement, the forecast ledger, and all SQLite state. If the sidecar dies, nothing about the user's money is ambiguous.

This split isn't just tidy — it means the safety-critical path (sizing, limits, execution) lives entirely in the codebase you control tightly, in a memory-safe language, unaffected by AGPL, and testable without Python.

---

## 3. AGPL-3.0 Licensing Strategy

Fincept Terminal is licensed under **AGPL-3.0**. This section is placed early because it constrains the architecture, the repo layout, and the distribution story. (Standard disclaimer: this is engineering-level license analysis, not legal advice; get a lawyer's review before commercial distribution.)

### 3.1 What AGPL-3.0 Requires

1. **Derivative works must be AGPL-3.0.** Any program that incorporates or links Fincept code must be released under AGPL-3.0 with full source.
2. **Distribution triggers source obligations.** Shipping the sidecar binary to users (inside the Kalshi Monster installer) is distribution — users must be able to get the sidecar's complete corresponding source.
3. **§13 network clause.** If the modified code runs as a network service, users interacting with it remotely must also be offered the source. For a localhost sidecar this is mostly moot (the only "user" is the local app), but it becomes live if a hosted option (Option E) is ever pursued.

### 3.2 Compliance Design

**Rule 1 — Two repos, two licenses.**

- `fincept-sidecar` — a **public repository under AGPL-3.0**, containing all extracted/adapted Fincept Python code plus the FastAPI wrapper. Its `NOTICE` file credits the upstream Fincept Terminal project and links to it. Every modification to Fincept code stays in this repo.
- `kalshi-monster` — the existing app repo, under whatever license you choose (proprietary is fine). It contains **zero lines of Fincept-derived code**: no copied Python, no translated-to-Rust Fincept algorithms, no copied prompt templates. Its only knowledge of the sidecar is the HTTP API contract (a plain OpenAPI schema, which the sidecar repo publishes and the app repo vendors as a spec file — API descriptions are not copyrightable expression).

**Rule 2 — Arm's-length interaction only.** The FSF's own guidance treats separate programs communicating over sockets with structured data as independent works rather than one combined program, provided the communication is genuinely arm's-length (no shared memory, no intimate data-structure exchange, either side replaceable). The design honors this: the API exchanges plain JSON, the Rust side would work identically against a clean-room reimplementation of the same API, and the sidecar can serve any client. Shipping both in one installer is *aggregation*, not combination — permitted, as long as Rule 3 is met.

**Rule 3 — Source availability at distribution.** The Kalshi Monster installer/about screen includes: the sidecar's license text, a link to the public `fincept-sidecar` repo pinned to the exact shipped commit, and (belt-and-suspenders) a written offer of source. CI enforces that every shipped sidecar binary embeds its git SHA and that the SHA exists as a public tag.

**Rule 4 — Watch the contamination vectors.** The subtle ways this goes wrong, each with a guard:

| Vector | Guard |
|---|---|
| Copying a Fincept prompt template or scoring formula into Rust "because it's small" | Code-review rule: anything derived from Fincept lands in the sidecar repo, period |
| PyO3/embedding "just for one fast path" later | Architecture decision record; Option B is rejected permanently absent relicensing |
| An LLM-assisted "translation" of Fincept Python to Rust | A translation is a derivative work. If a Rust-native version is ever wanted, it must be a genuine clean-room reimplementation from the API spec and public domain finance math, by someone who hasn't read the Fincept source — or the Rust code goes AGPL too |
| Static assets (agent prompt files, config schemas) copied across repos | Same rule as code |

**Rule 5 — The escape hatches, ranked.** If AGPL obligations ever become unacceptable: (a) ask Fincept's author about dual licensing — many single-maintainer AGPL projects sell commercial licenses; (b) shrink the sidecar to only the pieces with no non-AGPL equivalent, replacing the rest with permissively-licensed libraries (yfinance is Apache-2.0, empyrical is Apache-2.0, pandas/scipy are BSD — much of Fincept's value is *composition* of permissive libraries, and independent recomposition against those libraries directly is not a derivative of Fincept); (c) full Rust rewrite (Option D).

Point (b) deserves emphasis: a significant fraction of the plan's value — yfinance market data, empyrical metrics, EconDB REST calls, standard TA indicators — can be implemented against the underlying permissive libraries without deriving from Fincept at all. The pragmatic strategy is to **use Fincept's code where it embodies real, hard-won logic (the 9-agent framework, its prompts and aggregation) and write thin original code for the commodity data plumbing.** That keeps the AGPL surface small from day one.

---

## 4. The Edge Engine: Model Probability vs. Market Price

This is the heart of the system, and it needs more rigor than v1.0 gave it. Terminology first: comparing a model's probability against the market's implied probability is **model-based value betting**, not arbitrage — there is no riskless leg. (True cross-venue arbitrage, e.g. Kalshi vs. Polymarket on equivalent events, is a different and worthwhile future capability; see 4.6.)

### 4.1 The Market Price Is a Strong Opponent

A Kalshi YES price of 72¢ is not noise to be corrected — it is the aggregated opinion of self-selected traders with money at stake, and on liquid markets it is well-calibrated. The prior for any new model must be: **the market is right and we are wrong.** The engine encodes this humility as *shrinkage*: the tradable probability is a weighted blend of model and market, in log-odds space, with the model's weight earned through demonstrated calibration:

```
logit(p) = ln(p / (1 − p))

logit(p_final) = λ · logit(p_model) + (1 − λ) · logit(p_market)
```

- `λ` starts at **0.25** (the model only nudges the market price) and is re-fit quarterly from the forecast ledger: λ is chosen to minimize Brier score of `p_final` over resolved forecasts. If the model is junk, λ collapses toward 0 and the system correctly stops finding "edge."
- Blending in log-odds space rather than probability space matters at the tails: averaging 0.95 and 0.70 as probabilities ignores that 0.95 is a much stronger claim than the arithmetic suggests.

### 4.2 Costs: Fees, Spread, and Adverse Selection

Kalshi's trading fee is approximately `fee(p) = 0.07 · p · (1 − p)` dollars per contract (rounded up to the cent), maximized at p = 0.50 where it costs 1.75¢ per contract. On top of that:

- **Spread:** you buy at the ask, not the mid. On thin markets the half-spread can be 2–5¢.
- **Adverse selection:** a resting order that fills instantly often fills because someone knew more. Treat instant fills on stale quotes with suspicion.

The engine therefore computes **net edge**, never raw edge:

```
p_entry     = ask + fee(ask)                  # effective cost per $1 payout, buying YES
edge_net    = p_final − p_entry               # for YES; symmetric for NO
```

**Trade threshold:** `edge_net ≥ θ`, with θ starting at **0.05 (5¢)** and never below 0.03. A 5¢ minimum sounds conservative; it is the difference between a system that trades constantly and bleeds fees, and one that trades rarely and wins. θ can come down as calibration evidence accumulates.

### 4.3 The Analysis Pipeline for One Market

```
Kalshi market (ticker, rules, close date, order book)
   │
   ├─ 1. RESOLUTION PARSING (Rust + LLM assist)
   │     Extract: the exact resolution criterion, data source, deadline,
   │     and edge cases. THE #1 LOSS SOURCE IN PREDICTION MARKETS IS
   │     MISREADING THE RULES, not mispricing. The LLM produces a
   │     structured summary; anything ambiguous flags the market DO-NOT-TRADE.
   │
   ├─ 2. CLASSIFICATION → which agents apply (Section 5 routing table)
   │     economic | index/price-level | company-event | political | other
   │
   ├─ 3. AGENT RUNS (Python sidecar, parallel)
   │     Each applicable agent returns: probability, confidence, rationale
   │
   ├─ 4. AGGREGATION (Rust) → p_model
   │     Weighted log-odds pooling across agents (Section 5.3),
   │     with a disagreement penalty on confidence
   │
   ├─ 5. SHRINKAGE (Rust) → p_final = blend(p_model, p_market, λ)
   │
   ├─ 6. COST MODEL (Rust) → edge_net vs. ask + fee + spread
   │
   ├─ 7. VERDICT
   │     edge_net ≥ θ and confidence ≥ minimum → candidate trade
   │     → Kelly sizing (Section 6) → forecast ledger entry → user card
   │     otherwise → logged as PASS with reasons (passes are calibration data too)
```

Everything from step 4 down runs in Rust: aggregation, shrinkage, costs, sizing. The sidecar supplies opinions; the Rust core decides.

### 4.4 Worked Example (replacing v1.0's hand-wave)

Market: *"Will July CPI YoY exceed 3.0%?"* — YES ask 72¢, bid 70¢.

1. **Resolution parsing:** resolves against BLS CPI-U YoY, release Aug 12, rounded to one decimal. Unambiguous → tradable.
2. **Classification:** economic → Macro (weight 0.5), News (0.2), Sentiment (0.1), Technical-on-contract (0.2).
3. **Agents:** Macro agent uses EconDB: trailing CPI 3.4%, 4 straight months above 3.0%, energy base effects fading, nowcast 3.2% with σ ≈ 0.15 → P(>3.0) ≈ 0.91. News agent: no disinflation catalysts pre-release → 0.85. Sentiment: mildly hawkish chatter → 0.80 (low confidence). Contract price action: drifting up on volume → 0.78.
4. **Aggregation:** weighted log-odds pool → `p_model ≈ 0.87`.
5. **Shrinkage** (λ = 0.25, p_market = mid 0.71): `logit(p_final) = 0.25·logit(0.87) + 0.75·logit(0.71)` → **p_final ≈ 0.756**.
6. **Costs:** entry = 0.72 ask + fee(0.72) ≈ 0.72 + 0.0142 = **0.734**.
7. **Verdict:** edge_net = 0.756 − 0.734 = **+2.2¢ → PASS** (below the 5¢ threshold).

Note what happened: the raw model said 87% vs. a 72¢ market — v1.0's logic would have called that a fat +15% edge and bet heavily. After shrinkage and costs, the honest number is +2.2¢, below threshold. Most "edges" die exactly this way, and *that is the system working correctly*. The trades that survive this gauntlet are the ones worth real money.

### 4.5 Failure-Mode Checklist (evaluated per candidate trade)

| Failure mode | Automated check |
|---|---|
| Misread resolution rules | LLM rule summary must parse into structured fields; ambiguity → DO-NOT-TRADE |
| Stale model inputs | Every agent response carries data timestamps; any input older than its freshness budget voids the run |
| Thin market / phantom liquidity | Minimum open interest and max spread filters (configurable; defaults OI ≥ 500, spread ≤ 5¢) |
| Correlated duplicate exposure | Ledger check: does the portfolio already hold this event in another form? (Section 6.3) |
| Time-value blindness | Edge must be annualized-adjusted: 3¢ of edge locking capital for 6 months ≠ 3¢ resolving Friday |
| News between analysis and click | Quotes re-fetched at order preview; if the ask moved > 1¢ against us, re-run steps 5–7 |

### 4.6 Future: True Cross-Venue Arbitrage

Once the edge engine is stable, the same plumbing supports genuine arbitrage: equivalent contracts on Kalshi vs. Polymarket where `YES_A + NO_B < $1` after fees. This is a different discipline (execution speed and contract-equivalence verification dominate; probability modeling is irrelevant) and is deliberately out of scope until Phase 5 execution is proven. It's noted here so the `EdgeEngine` API is designed with a second venue in mind (venue-tagged quotes, not Kalshi-shaped structs in core logic).

---

## 5. Mapping the 9 AI Hedge Fund Agents to Prediction Markets

Fincept's agents were built to answer "should I buy this stock?" Prediction markets ask a sharper question — "what is the probability of this event?" — and every agent needs an adaptation layer. This section is the design for that layer: per-agent role, inputs, output contract, and when the agent is routed in.

**Universal output contract** (every agent, enforced by Pydantic schema):

```python
class AgentSignal(BaseModel):
    agent: str                      # "macro", "valuation", ...
    probability: float | None      # P(YES) in [0.01, 0.99]; None = "no opinion"
    confidence: float               # [0, 1] self-assessed reliability
    rationale: str                  # 2-5 sentences, shown to user
    inputs_used: list[DataRef]      # source + timestamp for every input (staleness checks)
    caveats: list[str]              # explicit unknowns
```

Agents must return `probability=None` rather than a hallucinated number when they lack relevant data — a sentiment agent has no business opining on "will initial jobless claims exceed 230k."

### 5.1 The Nine Agents, Adapted

**1. Macro Agent — the workhorse for Kalshi's bread-and-butter markets.**
Equity role: assess the macro regime as context. Prediction-market role: **direct probability estimation for economic contracts** — the largest, most liquid Kalshi category. For "CPI above X" / "Fed cuts in September" / "GDP growth above Y" contracts, the resolution variable *is* an EconDB series. The agent builds a nowcast: trailing values, momentum, consensus forecasts, base effects, and known calendar events, producing a distribution over the release value and reading P(threshold) off it. For non-economic markets it degrades to its equity role: regime context that conditions other agents (e.g., high-VIX regimes widen every distribution).

**2. Technical Analyst — distributional forecasts for price-level contracts.**
Equity role: 5-strategy trade signals (trend, mean reversion, momentum, vol regime, stat-arb). Prediction-market role: for contracts of the form "will \<index/asset\> close above K by T," convert price dynamics into `P(S_T > K)` — structurally the same computation as pricing a binary option:

```
P(S_T > K) ≈ Φ( (ln(S/K) + (μ − σ²/2)·τ) / (σ·√τ) )
```

with σ from realized vol (upgraded to implied vol where available — for S&P contracts, the VIX term structure is free information the market may not have fully priced into Kalshi), μ conservatively ≈ 0, and the momentum/mean-reversion strategies contributing small drift adjustments. **Second, novel role:** run technical analysis *on the Kalshi contract's own price series* — prediction-market prices exhibit momentum and longshot bias, and the contract's own tape is a signal none of the equity-oriented agents would think to read.

**3. Valuation Agent — company-event contracts only.**
Equity role: DCF and Owner Earnings intrinsic value. Prediction-market role: narrow but real — for contracts tied to company outcomes ("TSLA above $300 at year-end," "will X announce Y by Q3"), the DCF machinery produces a fair-value anchor and the gap between anchor and spot informs drift in the Technical agent's distribution. Routed in *only* for company-linked markets; returns `None` otherwise.

**4. Fundamentals Agent — company health as event probability.**
Equity role: profitability/growth/health scoring. Prediction-market role: for earnings-adjacent and company-event contracts, financial health translates to event probabilities (a company with deteriorating cash flow is likelier to guide down, cut dividends, miss targets). Also supplies base rates: historical beat/miss frequencies conditional on fundamentals profile.

**5. Sentiment Agent — crowd positioning, used contrarian-aware.**
Equity role: news/social sentiment scoring. Prediction-market role: two-sided. Directionally, sentiment shifts precede price moves on event contracts. But on politically charged markets, retail sentiment is *the thing mispricing the contract* — partisan money famously distorts political markets. The adaptation: sentiment is reported alongside a **crowd-bias flag**; on flagged categories (elections, hot-button policy), the aggregator inverts the usage — strong one-sided retail sentiment *against* our model's direction slightly *increases* confidence that the market price is distorted.

**6. News Agent — resolution-relevant facts and catalysts.**
Equity role: headline summarization and impact. Prediction-market role: the sharpest adaptation of all — the agent is prompted with the market's *resolution criterion* and asked: "what recent news bears on this exact criterion before this exact deadline?" Scheduled announcements (Fed meetings, earnings dates, data releases) inside the contract window are extracted as **catalyst events** with timestamps; a contract that looks mispriced but has a catalyst tomorrow is a different bet from the same price with no catalyst. Also powers the position-monitoring alerts in Phase 4.

**7. Risk Manager — from single-stock risk to correlated-binary portfolios.**
Equity role: position sizing and concentration limits. Prediction-market role: the deepest rewrite, because binary contracts break equity risk intuitions: maximum loss is total (the stake) and outcomes cluster in *time* (many contracts resolve on the same CPI print or the same election night). The agent computes: per-event exposure (all contracts resolving on the same underlying event, netted by direction), resolution-date clustering (worst-case single-day loss), and a correlation matrix over open positions using shared-driver tagging (Section 6.3). Its output caps and scales the Kelly sizes — it can veto, never enlarge.

**8. Portfolio Manager Agent — demoted from decider to advisor.**
Equity role: aggregate all signals into buy/sell/hold. Prediction-market role: **deliberately reduced.** Final aggregation must be deterministic, auditable Rust code (Section 5.3), not an LLM's judgment — you cannot calibrate a system whose final step is a different LLM whim each run. The agent instead performs *qualitative review* of the deterministic result: sanity-check the verdict against the rationales, flag inconsistencies ("the News agent found a catalyst tomorrow but the Technical agent's forecast assumes no jumps"), and recommend the analysis-depth tier for follow-up. Its flags reduce confidence; they never change p_model.

**9. Explainability Agent — the audit trail that makes calibration reviewable.**
Equity role: human-readable rationale. Prediction-market role: unchanged in spirit, elevated in importance. Every ledger entry gets a structured explanation: per-agent probabilities and weights, the shrinkage math, the cost breakdown, and the decisive factors. When the quarterly calibration review finds a bucket of bad forecasts, these records are what make the failure *diagnosable* ("every miss in March shared the same stale-EconDB-nowcast input") instead of a mystery.

### 5.2 Routing Table

| Market category | Agents (weight in pool) |
|---|---|
| Economic data releases (CPI, jobs, GDP, rates) | Macro 0.50 · News 0.20 · Technical-on-contract 0.20 · Sentiment 0.10 |
| Index / asset price levels | Technical 0.45 · Macro 0.25 · News 0.15 · Sentiment 0.15 |
| Company events | Fundamentals 0.30 · Valuation 0.25 · News 0.25 · Sentiment 0.10 · Technical 0.10 |
| Political / elections | News 0.35 · Sentiment 0.30 (bias-flagged) · Macro 0.15 · Technical-on-contract 0.20 |
| Weather / science / other | News 0.50 · Technical-on-contract 0.30 · Sentiment 0.20 |

Risk Manager, Portfolio Manager, and Explainability run on every candidate; they shape sizing and reporting, not p_model. Weights are config, not code — they will be re-fit from the ledger once there's data.

### 5.3 Deterministic Aggregation (Rust)

```rust
/// Weighted log-odds pooling with a disagreement penalty.
pub fn aggregate(signals: &[AgentSignal], weights: &HashMap<Agent, f64>) -> ModelOpinion {
    let opining: Vec<_> = signals.iter()
        .filter(|s| s.probability.is_some())
        .collect();

    // Effective weight = routing weight × agent's self-assessed confidence
    let mut wsum = 0.0;
    let mut pooled_logit = 0.0;
    for s in &opining {
        let w = weights[&s.agent] * s.confidence;
        pooled_logit += w * logit(s.probability.unwrap().clamp(0.01, 0.99));
        wsum += w;
    }
    let p_model = sigmoid(pooled_logit / wsum);

    // Disagreement penalty: high variance across agents = low ensemble confidence.
    let probs: Vec<f64> = opining.iter().map(|s| s.probability.unwrap()).collect();
    let spread = std_dev(&probs);
    let confidence = (1.0 - (spread / 0.25).min(1.0)) * mean_confidence(&opining);

    ModelOpinion { p_model, confidence, contributions: per_agent_breakdown(&opining) }
}
```

Two properties worth the boilerplate: the same inputs always give the same output (calibratable), and `contributions` records each agent's pull on the final number (attributable — the quarterly review can discover "the Sentiment agent has been pure noise; cut its weight").

---

## 6. Risk Management and Position Sizing

v1.0's Kelly enhancement multiplied ad-hoc factors together (`concentration_factor × correlation_factor × vol_factor`), which composes badly — three reasonable-looking 0.7 haircuts silently become 0.34, and no one can say what the resulting number means. v2.0 restructures sizing as **one honest Kelly computation, then explicit caps applied as hard limits with named reasons.**

### 6.1 Kelly for a Binary Contract, Done Right

For buying YES at effective entry price `c` (ask + fee) with true probability `p`, the Kelly-optimal fraction of bankroll is:

```
f* = (p − c) / (1 − c)
```

(derivation in Appendix C). The system stakes **quarter-Kelly**: `f = 0.25 · f*`, using `p = p_final` (the *shrunk* probability — using raw p_model here would double-count our self-confidence). Quarter rather than half because our p is estimated, not known, and Kelly's penalty for overestimating p is brutally asymmetric: betting 2× true Kelly has zero expected growth; betting under merely grows slower. When in doubt, size down — the bankroll's survival is the product, not any single bet.

### 6.2 Hard Caps (applied after Kelly, each with a user-visible name)

| Cap | Default | Rationale |
|---|---|---|
| Per-position | 5% of bankroll | Binary outcomes: max loss = stake |
| Per-event | 8% | All contracts resolving on the same underlying event, summed |
| Per-resolution-day | 15% | CPI day / election night clustering — worst single-day loss bound |
| Per-category | 25% | e.g., all economics, all politics |
| Existing 10% app cap | retained | Belt and suspenders |
| Open-position count | 15 | Attention is a risk resource; unmonitored positions rot |

The order card shows which cap bound the stake ("Kelly suggested $210; capped at $150 by per-event limit — you already hold FED-SEP-CUT").

### 6.3 Correlation Without a Covariance Matrix

Binary event contracts don't come with return covariances, so the Risk Manager approximates via **shared-driver tagging**: every analyzed market is tagged with its drivers (`inflation`, `fed-policy`, `sp500-level`, `election-2026`, …) at classification time. Two positions correlate if they share a driver; exposure per driver is summed and capped (default 10% of bankroll net per driver, counting direction — long "CPI > 3.0" and long "Fed holds in Sept" are the *same bet* wearing two tickers, which v1.0's math would happily have double-sized).

### 6.4 Circuit Breakers (Rust, no sidecar dependency)

| Trigger | Action |
|---|---|
| Daily realized loss > 5% of bankroll | New orders paused until next day; notification sent |
| Drawdown from high-water mark > 15% | All stakes scaled ×0.5 until drawdown < 10% |
| Drawdown > 25% | Live trading disabled; requires manual re-enable in Settings |
| Calibration degradation (rolling-50 Brier worse than market's by > 0.02) | System reverts to paper-trading mode automatically |

That last row is the one v1.0 had no concept of: the system continuously proves it deserves to trade, and demotes itself when it stops proving it.

### 6.5 What the Money Never Does

Written as invariants, enforced in the Rust core, unit-tested:

1. No order without explicit per-order user confirmation (no auto-trading, ever, in this plan's scope).
2. No order while any circuit breaker is tripped.
3. No order sized above the minimum of all applicable caps.
4. No order on a market flagged DO-NOT-TRADE by resolution parsing.
5. No live order before the Phase 3 calibration gate is passed (enforced by config that Phase 5 code checks, not by discipline).

---

## 7. Phased Implementation

Restructured from v1.0's seven phases + polish (24 weeks) to six phases (~18 weeks) plus an explicit backlog. Each phase has **exit criteria** — verifiable statements, not vibes. Biggest sequencing changes: a new Phase 0 (fix what exists), calibration promoted to its own phase *before* execution, and macro/EconDB pulled *earlier* (it's core to the edge engine, not a Phase-5 garnish), while sentiment/alt-data drops to backlog.

### Phase 0 — Revive the Dead Code (Weeks 1–2, no Fincept required)

The highest-ROI work in this plan touches zero new infrastructure.

- [ ] **Live market board:** replace the 5 hardcoded demo props with live Kalshi markets from the existing client (it already does search and category browsing — the data is *right there*)
- [ ] **Wire `MarketContextBuilder`:** call `build_context_packet()` in the chat pipeline so the LLM sees real Kalshi market state (fixing this is a one-day change with immediate quality impact)
- [ ] **Decide `ml.rs`:** delete it. The logistic regression has no training data and the agent pipeline supersedes it. Dead code is a tax; if a local model is wanted later, it will be designed around the forecast ledger's data
- [ ] **Forecast ledger:** the table every later phase depends on:

```sql
CREATE TABLE forecasts (
    id INTEGER PRIMARY KEY,
    market_ticker TEXT NOT NULL,
    created_at TEXT NOT NULL,           -- ISO 8601
    close_time TEXT NOT NULL,
    p_market REAL NOT NULL,             -- mid at analysis time
    p_model REAL,                       -- NULL until Phase 2 agents exist
    p_final REAL NOT NULL,
    verdict TEXT NOT NULL,              -- 'trade_yes' | 'trade_no' | 'pass'
    verdict_reasons TEXT NOT NULL,      -- JSON array
    stake_suggested REAL,
    agent_breakdown TEXT,               -- JSON: per-agent p, confidence, weight
    -- filled at resolution:
    resolved_at TEXT,
    outcome INTEGER,                    -- 1 = YES, 0 = NO
    brier_model REAL, brier_market REAL, brier_final REAL
);
```

- [ ] **Resolution poller:** background task that checks open ledger entries against Kalshi's settlement data and fills the outcome columns

**Exit criteria:** app displays live Kalshi markets; every chat answer includes real market context; every recommendation (even manual/chat-driven ones) writes a ledger row; resolved markets get outcomes recorded automatically.

### Phase 1 — Sidecar Foundation (Weeks 3–5)

- [ ] Create the public `fincept-sidecar` repo (AGPL-3.0, NOTICE crediting upstream) with the layout:

```
fincept-sidecar/
  pyproject.toml
  main.py                  # FastAPI app; reads FINCEPT_PORT + FINCEPT_TOKEN from env
  api/v1/                  # routers: market, agents, economic, portfolio
  engines/                 # thin original wrappers over yfinance/EconDB (permissive-licensed path)
  agents/                  # Fincept-derived agent code (the AGPL heart)
  models/schemas.py        # Pydantic contracts, exported as OpenAPI for the Rust side
```

- [ ] Startup handshake (solves v1.0's fixed-port fragility):

```python
# main.py
import os, secrets, sys, uvicorn
from fastapi import FastAPI, Request, HTTPException

TOKEN = os.environ["FINCEPT_TOKEN"]          # generated by Rust per launch
app = FastAPI()

@app.middleware("http")
async def auth(request: Request, call_next):
    if request.headers.get("authorization") != f"Bearer {TOKEN}":
        raise HTTPException(401)
    return await call_next(request)

if __name__ == "__main__":
    port = int(os.environ.get("FINCEPT_PORT", "0"))   # 0 = OS-assigned
    config = uvicorn.Config(app, host="127.0.0.1", port=port)
    server = uvicorn.Server(config)
    # print the bound port on stdout so the parent can read it
    @app.on_event("startup")
    async def announce():
        actual = server.servers[0].sockets[0].getsockname()[1]
        print(f"FINCEPT_READY port={actual}", flush=True)
    server.run()
```

- [ ] Rust bridge that spawns, handshakes, supervises, and kills:

```rust
pub struct FinceptBridge {
    child: Option<CommandChild>,       // tauri-plugin-shell child
    base_url: OnceCell<String>,
    token: String,                     // random per launch
    client: reqwest::Client,           // 30 s default timeout, per-call overrides
}

impl FinceptBridge {
    pub async fn start(&mut self, app: &AppHandle) -> Result<()> {
        self.token = generate_token();
        let (mut rx, child) = app.shell()
            .sidecar("fincept-sidecar")?
            .env("FINCEPT_TOKEN", &self.token)
            .env("FINCEPT_PORT", "0")
            .spawn()?;
        self.child = Some(child);
        // Read stdout until FINCEPT_READY or 30 s timeout
        let port = wait_for_ready_line(&mut rx, Duration::from_secs(30)).await?;
        self.base_url.set(format!("http://127.0.0.1:{port}")).ok();
        Ok(())
    }

    /// Supervision: on 3 consecutive failed health checks, kill + restart,
    /// max 3 restarts per 10 minutes, then mark degraded and notify the UI.
    pub fn spawn_supervisor(self: Arc<Self>, app: AppHandle) { /* tokio task */ }
}
```

- [ ] Market data endpoints (`/market/tracker`, `/market/price/{ticker}`, `/market/history/{ticker}`, `/market/search`) built on yfinance directly (thin original code — keeps this off the AGPL-critical path per Section 3, Rule 5b)
- [ ] "World Markets" tab in React; market snapshot feeding `MarketContextBuilder`
- [ ] Degradation test in CI: kill -9 the sidecar mid-session → app keeps working on Kalshi-only features, UI shows a "analysis engine offline" badge, supervisor restarts it

**Exit criteria:** sidecar lifecycle is invisible to the user (starts with app, dies with app, restarts on crash); live cross-asset prices appear in chat context; the kill-test passes.

### Phase 2 — Agents and the Edge Engine (Weeks 6–9) — *the core of the project*

- [ ] Port the 9 agents into the sidecar with the `AgentSignal` contract and prediction-market adaptations from Section 5
- [ ] EconDB integration **now, not Phase 5** — the Macro agent is useless for Kalshi's economic contracts without it (`/economic/{country}/{indicator}`, `/economic/macro/snapshot`)
- [ ] `POST /agents/market-opinion` — the one endpoint that matters most:

```python
class MarketOpinionRequest(BaseModel):
    market_ticker: str
    title: str
    resolution_rules: str            # full text; agents receive the exact criterion
    close_time: datetime
    category: MarketCategory         # classified by the Rust side
    yes_bid: float; yes_ask: float
    context: dict                    # related tickers, OI, recent contract candles

class MarketOpinionResponse(BaseModel):
    signals: list[AgentSignal]
    catalysts: list[CatalystEvent]   # dated events inside the contract window
    elapsed_ms: int
```

- [ ] Rust `EdgeEngine`: classification, aggregation (5.3), shrinkage (4.1), cost model (4.2), verdict + ledger write (4.3). Fee math property-tested against Kalshi's published schedule
- [ ] Resolution-parsing step with DO-NOT-TRADE flagging
- [ ] "Edge Board" UI: ranked candidate list — market, p_market, p_final, net edge, confidence, per-agent breakdown drawer, and PASS entries with reasons (seeing why things *don't* qualify builds trust in why things do)
- [ ] Depth tiers: `quick` (cached data, no LLM agents, < 2 s) for board scanning; `standard` (~30 s) for candidates; `deep` (all agents + fresh news) on demand

**Exit criteria:** any Kalshi market → full analysis in < 30 s (standard tier) with p_model, p_final, net edge, and per-agent attribution; every analysis writes a ledger row; the worked example in 4.4 is reproducible end-to-end.

### Phase 3 — Calibration and Paper Trading (Weeks 10–12) — *the gate*

- [ ] Auto-analysis of a configurable market universe (default: top 50 by volume across economics + indices) on a schedule, populating the ledger without user action — this is what accumulates sample size fast
- [ ] Paper portfolio: verdicts ≥ threshold become simulated positions at real ask prices with real fee math
- [ ] Calibration dashboard: reliability diagram (10 buckets), Brier score of p_model / p_market / p_final, paper P&L vs. always-pass baseline, per-agent hit rates
- [ ] Quarterly (initially monthly) re-fit job: λ, routing weights, and θ from ledger data
- [ ] **The gate, in code:** live execution (Phase 5) reads `calibration_gate_passed`, which flips only when: ≥ 200 resolved forecasts AND Brier(p_final) ≤ Brier(p_market) AND paper P&L after fees > 0 over the window

**Exit criteria:** the dashboard exists and is honest; the gate is enforced in code; you can answer "does this system actually predict better than the market?" with data instead of hope. **If the answer is no, Phases 4–5 wait, and that is the plan working, not failing.** The fallback posture if the gate never passes: the app remains a top-tier research/context tool (which is already far beyond v0.8.0) without live sizing.

### Phase 4 — Portfolio Risk (Weeks 13–15)

- [ ] Shared-driver tagging in classification; driver-exposure accounting (6.3)
- [ ] Kelly + caps + circuit breakers (6.1–6.4) in the Rust core, fully unit-tested including the invariants in 6.5
- [ ] Portfolio tab: unified view (Kalshi positions + paper positions + ledger history), empyrical metrics via sidecar (`POST /portfolio/analyze`), exposure-by-driver chart
- [ ] Position monitoring: News agent runs on open positions' drivers on a schedule; material developments → Discord/Telegram alert with the affected position and suggested review

**Exit criteria:** every open position shows its driver tags and contribution to caps; a simulated correlated-overexposure scenario is blocked by the caps in a test; circuit breakers fire correctly in simulation.

### Phase 5 — Execution (Weeks 16–18, gated on Phase 3)

- [ ] Implement `KalshiClient::place_order()` — against **Kalshi's demo environment first** (`demo-api.kalshi.co`), then production
- [ ] Order preview card: market, side, contracts, limit price, max loss, fee, which cap bound the size, and the edge math that justified it; explicit confirm required
- [ ] Re-quote check at confirm time (4.5, last row); order status tracking → ledger + notifications
- [ ] Execution invariants (6.5) wired to real order flow; full E2E on demo: analyze → verdict → preview → confirm → fill → track → resolve → ledger

**Exit criteria:** a real (demo) order round-trips end to end; every invariant in 6.5 has a test that tries to violate it and fails.

### Phase 6 — Expansion (ongoing, post-core)

In priority order, pulled from the backlog as capacity allows: GNews/RSS news alerts wired to the alert engine → multi-LLM routing (Gemini/OpenAI/Ollama; Ollama earns its place for cost — the auto-analysis loop in Phase 3 makes hundreds of LLM calls daily, and routing `quick`-tier scans to a local model changes the economics) → robo-advisor/screening tab → sentiment expansion → cross-venue arbitrage scaffolding (4.6) → **the stocks-and-crypto expansion (Phases 7–9, scoped in full in Section 14)**.

### Explicit Backlog (moved out of the phase plan, with reasons)

| v1.0 item | Why it's backlog |
|---|---|
| YouTube sentiment (RoBERTa + transcripts) | ~2 GB of the bundle-size problem for the least-validated signal in the stack; revisit if per-agent hit rates show sentiment pulling weight |
| Google Scholar consumer research | No plausible path to a Kalshi edge |
| DataGovIN India data | No liquid Kalshi markets resolve on Indian data |
| Fyers broker (Indian equities) | Wrong geography; the equity execution path is the Section 14 roadmap (Alpaca-first) |
| Robinhood connector | Superseded — Section 14 selects Alpaca for equity/crypto execution (official API, first-class paper environment); Robinhood's API is unofficial and unsuitable for the money path |
| Fincept community forum | Not a trading capability |

---

## 8. Data Flow Architecture

```
EXTERNAL SOURCES                      SIDECAR (Python, AGPL repo)
  Kalshi API ────────────┐             ┌──────────────────────────┐
  yfinance ──────────────┼──(quotes,   │ MarketDataEngine (cache) │
  EconDB ────────────────┤   history,  │ 9 Agents (parallel runs) │
  GNews / RSS ───────────┤   macro,    │ EconDB client            │
  LLM providers ─────────┘   news)────▶│ Portfolio analytics      │
                                       └────────────┬─────────────┘
                                          AgentSignals, data (JSON)
                                                     │  bearer-token HTTP
                                                     ▼
                                       RUST CORE (decisions & money)
                                        Classification → Aggregation
                                        → Shrinkage → Cost model
                                        → Verdict → Kelly + caps
                                        → Forecast ledger (SQLite)
                                        → Order flow (Kalshi only, gated)
                                        → Alerts (Discord/Telegram)
                                                     │ Tauri commands
                                                     ▼
                                       REACT UI
                                        Edge Board · Chat (context-injected)
                                        Portfolio · Calibration dashboard
                                        World Markets · Settings
```

Note the deliberate asymmetry versus v1.0's diagram: **the Kalshi API is called only from Rust.** The sidecar never sees Kalshi credentials and physically cannot place orders. All money-adjacent state lives in one place.

---

## 9. Frontend Plan

New views, in build order:

| View | Phase | Content |
|---|---|---|
| **Edge Board** (replaces the demo prop board as the home tab) | 2 | Ranked analyzed markets: p_market vs. p_final, net edge, confidence, verdict; per-agent breakdown drawer; PASS list with reasons |
| **World Markets** | 1 | 132-instrument snapshot, 6 category filters |
| **Calibration** | 3 | Reliability diagram, Brier trends, paper P&L, per-agent hit rates, gate status |
| **Portfolio** (rebuilt) | 4 | Unified positions, driver-exposure chart, empyrical metrics, cap utilization |
| **Research** | 6 | Stock research station (info/technicals/fundamentals/quant/news) |

Enhancements to existing views: **Chat** gains auto-injected market + macro context and an "analyze this market" affordance that routes to the edge pipeline; **Kalshi Markets** rows gain a one-click `quick`-tier opinion; **Settings** gains sidecar status/logs, new API keys (EconDB, GNews, Gemini/OpenAI/Ollama), depth-tier defaults, and the risk-limit configuration (caps and breakers from Section 6, all user-visible and user-tightenable — never user-loosenable beyond defaults without a confirmation dialog).

One deliberate omission: v1.0 planned six new tabs at once. Tab count is not value. The Edge Board is the product; everything else supports it.

---

## 10. Technical Details

### 10.1 Sidecar Lifecycle

Covered in Phase 1's code: ephemeral OS-assigned port announced over stdout (no fixed-port collisions), per-launch bearer token, supervisor with bounded restarts (3 per 10 minutes, then degraded mode with a visible UI badge), graceful shutdown on app exit with a kill fallback. The app **must remain fully functional Kalshi-only when the sidecar is down** — this is a tested invariant, not an aspiration.

### 10.2 Security

- Sidecar binds `127.0.0.1` only; every request carries the per-launch bearer token (any local process can hit a localhost port — the token means only Kalshi Monster's requests are honored)
- Kalshi credentials never cross the bridge (Section 8); provider API keys are sent per-request to the sidecar rather than stored in it, so the sidecar keeps no secrets at rest
- No request/response bodies containing keys are logged on either side
- Windows packaging note: PyInstaller one-file executables trip antivirus heuristics constantly. Ship the sidecar as a one-*dir* bundle, and code-sign both binaries

### 10.3 Caching (in the sidecar, with staleness contracts)

| Data | TTL | Note |
|---|---|---|
| Market quotes | 60 s | In-memory, single-flight (concurrent requests for the same ticker share one fetch) |
| OHLCV history | 15 min | In-memory |
| Fundamentals / statements | 24 h | SQLite (sidecar-local cache db, distinct from the app's db) |
| EconDB indicators | 6 h, **but busted by release calendar** | A 6-hour-old CPI figure is fine except in the hour after a CPI release, when it's poison. The macro engine knows release timestamps and invalidates eagerly |
| News | 15 min | In-memory |
| Agent runs | 15 min, keyed on input hash | A re-run with identical inputs is free; any changed input misses |

Every cached value carries its fetch timestamp, which flows into `AgentSignal.inputs_used` — this is what makes the staleness check in 4.5 enforceable rather than decorative.

### 10.4 Packaging

- **Dev:** system Python ≥ 3.11, `uvicorn main:app` — hot reload works
- **Distribution:** PyInstaller one-dir bundle of the sidecar, listed in `tauri.conf.json` `externalBin`. Base bundle (no torch/transformers) targets **< 300 MB**. RoBERTa sentiment is an optional post-install download (the Sentiment agent falls back to LLM-based scoring without it) — this converts v1.0's ~2 GB problem into an opt-in
- CI builds the sidecar from a pinned tag of the public repo and embeds the git SHA (license traceability, Section 3 Rule 3)

### 10.5 Config Additions

```rust
pub struct AppConfig {
    // existing fields...
    // Sidecar
    pub sidecar_auto_start: bool,            // default true
    // Providers (all optional)
    pub econdb_api_key: Option<String>,
    pub gnews_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub ollama_base_url: Option<String>,
    // Edge engine
    pub shrinkage_lambda: f64,               // default 0.25; re-fit writes here
    pub min_edge_cents: f64,                 // default 5.0, floor 3.0
    pub default_depth: DepthTier,            // quick | standard | deep
    pub auto_analysis_universe: UniverseSpec,
    // Risk (Section 6 defaults; UI can tighten freely, loosen only past a confirm)
    pub risk_caps: RiskCaps,
    pub circuit_breakers: BreakerConfig,
    pub calibration_gate_passed: bool,       // written only by the Phase 3 job
}
```

(Port is no longer config — it's ephemeral by design.)

---

## 11. Testing and Calibration Discipline

v1.0 had no testing section. For a system that sizes real-money bets, that's not an oversight to patch — it's a philosophy to install:

- **Rust core (deterministic money path):** unit tests for fee math (property-based, vs. Kalshi's published schedule), Kelly + caps (including every 6.5 invariant expressed as a should-fail test), aggregation and shrinkage (golden vectors), ledger writes
- **Sidecar:** contract tests pinning the OpenAPI schema (a schema change that would break the Rust side fails CI in the sidecar repo first); agent smoke tests with recorded (VCR-style) data fixtures so tests don't hit yfinance/EconDB live
- **Integration:** spawn real sidecar → run the 4.4 worked example against fixtures → assert the ledger row; the kill -9 degradation test; the re-quote-at-confirm test
- **The permanent test:** the forecast ledger itself. Unlike any test suite, it evaluates the system against reality, forever. The quarterly review it feeds (re-fit λ, weights, θ; read per-agent attributions; investigate the worst bucket) is a scheduled maintenance activity of the product, not an optional analysis

---

## 12. Risks and Mitigations

| Risk | L × I | Mitigation |
|---|---|---|
| **The model never beats the market** — the entire premise fails | Med × High | This is why the calibration gate exists. Detected cheaply in Phase 3 with paper money; the fallback product (research terminal with live data and agent analysis) is still a large win over v0.8.0 |
| Misread resolution criteria cause confident wrong bets | Med × High | Structured rule parsing with DO-NOT-TRADE on ambiguity (4.3); post-mortems via Explainability records |
| AGPL contamination of the app repo | Low × High | Two-repo rule, review checklist, permissive-path plumbing (Section 3); legal review before commercial distribution |
| yfinance breaks or rate-limits (it's an unofficial API) | High × Med | Aggressive caching + single-flight; engine interface designed for a paid-provider swap (Polygon/Tiingo) behind the same endpoints |
| Sidecar crash loops | Med × Med | Bounded-restart supervisor, degraded mode, Kalshi-only fallback is a tested invariant |
| Agent latency makes the app feel broken | High × Med | Depth tiers; progressive per-agent streaming into the UI; auto-analysis pre-warms the board so users mostly read cached results |
| LLM cost blowup from auto-analysis | Med × Med | `quick` tier is LLM-free; Ollama routing for scans; per-day analysis budget in config |
| Windows AV flags the PyInstaller sidecar | High × Low | One-dir bundle + code signing (10.4) |
| Kalshi API/ToS changes around automated trading | Low × High | Execution is manual-confirm only; demo environment first; ToS review before Phase 5 ships |
| Scope creep back toward "integrate all 16 sources" | High × Med | Appendix B is the triage of record; anything leaving the backlog must name the edge it adds |

---

## 13. Known Gaps and Open Questions

Stated plainly rather than buried:

1. **Sports props are unsolved.** Fincept contributes nothing here. Options: (a) buy a sports data API (SportsDataIO, OpticOdds, etc. — real money) and build a proper props pipeline; (b) reposition the app as Kalshi-first and let props stay demo-grade until (a) is justified; (c) drop props. **This plan proceeds on (b)** and treats the props pipeline as a separate future plan with its own data-cost decision. The rebrand from "PrizePicks Monster" should reflect this.
2. **Fincept v4 extraction friction is unmeasured.** How cleanly the embedded-Python modules lift out of the v4 C++ host (vs. falling back to the v1.x pure-Python tree) is unknown until tried. Phase 1 includes a 2-day timeboxed spike; if extraction is ugly, the fallback is v1.x code + independent re-plumbing of data sources (which also shrinks the AGPL surface — Section 3 Rule 5b).
3. **EconDB tier limits.** Which indicators and country coverage the free/affordable tiers actually include determines how much of the Macro agent's Kalshi coverage is real. Verify before Phase 2 planning hardens; budget for a paid tier if needed.
4. **Kalshi ToS on assisted trading.** Manual-confirm flows are broadly fine, but the auto-analysis + suggested-orders pattern should be checked against current Kalshi API terms before Phase 5.
5. **λ and weight cold start.** Until ~200 forecasts resolve, shrinkage and routing weights are educated guesses. Mitigation: the auto-analysis loop (Phase 3) builds sample size even on markets never traded, and PASS verdicts count as forecasts too.

---

## 14. Future Work: Extending to Stocks and Crypto

This section scopes the expansion that motivates the Fincept integration in the first place: broadening from prediction markets into stocks and crypto as tradable asset classes, not just context data. It is written to the same standard as the rest of the plan — the math is derived, the costs are quantified, and nothing touches real money before a statistical gate. **Prerequisite: Phases 0–5 complete and the prediction-market calibration gate passed.** The prediction-market book is the proving ground precisely because binary contracts resolve in days and score unambiguously; asset forecasts take longer to validate, and the team should earn its process discipline on the fast-feedback domain first.

### 14.1 Why the Architecture Is Already Most of the Way There

The pipeline built in Phases 1–5 is asset-class-agnostic at every layer except the math:

| Pipeline stage | Prediction markets (built) | Stocks/crypto (Section 14) |
|---|---|---|
| Data | yfinance, EconDB via sidecar | **Same endpoints, same sidecar** — already fetching equity/crypto prices for context |
| Agents | Adapted to event probabilities (Section 5) | **Native mode** — this is what Fincept's agents were built for; the adaptation layer comes *off* |
| Model output | `p_model` (probability) | Return distribution: `(μ, σ, horizon)` |
| Prior / shrinkage | Market price in log-odds (4.1) | Equilibrium expected return (Black–Litterman-style; 14.3) |
| Cost hurdle | Fee + spread ≥ 5¢ edge (4.2) | Venue-specific round-trip cost hurdle (14.5) |
| Sizing | Binary Kelly `(p−c)/(1−c)`, quarter-Kelly | Continuous Kelly `μ/σ²`, quarter-Kelly, long-only cap (14.4) |
| Validation | Brier score vs. market, reliability diagram | IC / PIT / CRPS (14.6) — **Brier does not apply; this is the easiest place to get the math wrong** |
| Gate | ≥200 resolved forecasts, beat market Brier | Rolling IC t-stat ≥ 2 over ≥60 days × ≥50 names (14.6) |
| Execution | Kalshi, Rust core, manual confirm | Alpaca (equities + crypto), Rust core, manual confirm (14.7) |
| Ledger | `forecasts` table | New `asset_forecasts` table, same review cadence |

Two things carry over unchanged and non-negotiably: **all money-path code lives in the Rust core** (the sidecar produces forecasts, never orders — broker credentials never cross the bridge, exactly as with Kalshi), and **the AGPL boundary is unaffected** (Alpaca's API is called from Rust with original code; no Fincept code touches execution).

### 14.2 The Math That Changes, Part 1: Sizing (get this exactly right)

**Single asset.** For an asset whose excess return (over the risk-free rate) has mean `μ` and variance `σ²` per period, investing bankroll fraction `f` gives expected log-growth:

```
g(f) = r + f·μ − f²·σ²/2
```

Maximizing: `g′(f) = μ − f·σ² = 0`, so the growth-optimal fraction is

```
f* = μ / σ²
```

This is the continuous-payoff analog of the binary `(p−c)/(1−c)`, and it inherits the same brutal asymmetry: betting at `2f*` has **zero** expected excess growth, and beyond it growth goes negative. The fractional-Kelly discipline carries over with known, exact costs:

- Half-Kelly (`f*/2`) captures **75%** of the optimal excess growth rate at half the volatility: `g(f*/2) − r = ¾ · μ²/(2σ²)`.
- Quarter-Kelly (`f*/4`) captures **7/16 ≈ 44%**: `g(f*/4) − r = (7/16) · μ²/(2σ²)`.

Those fractions are worth internalizing: quarter-Kelly gives up more than half the growth — and it is still the right choice here, because `f*` is linear in `μ`, and `μ` is by far the noisiest estimate in all of finance. An error that doubles your μ estimate silently puts quarter-Kelly at true half-Kelly; the safety margin is not decoration.

**Worked numbers (sanity anchors):**

- Liquid equity: model says μ = 5%/yr excess, σ = 20% → `f* = 0.05/0.04 = 1.25` (leverage!), quarter-Kelly = **31%** of the asset book.
- BTC: model says μ = 20%/yr excess, σ = 60% → `f* = 0.20/0.36 ≈ 0.56`, quarter-Kelly = **14%**. Crypto's variance term dominates: even wildly bullish return forecasts produce moderate Kelly sizes, and any sizing logic that doesn't show this behavior is wrong.

**Hard constraints layered on top (in order):** quarter-Kelly → `f ≤ 1.0` per book (no leverage, no margin) → long-only (no shorting; crypto perps and funding-rate mechanics are explicitly out of scope for this roadmap) → per-position cap 10% of the asset book → per-driver caps shared with the prediction-market book (14.8).

**Multiple assets.** The vector generalization is `f⃗* = Σ⁻¹ μ⃗` (Σ = covariance matrix of excess returns). Two mandatory guards, because this formula is an error amplifier:

1. **Never invert a raw sample covariance matrix.** With 100 names and a year of daily data, `Σ̂⁻¹` amplifies estimation noise into extreme, unstable long-short weights. Use **Ledoit–Wolf shrinkage** (sample covariance shrunk toward a structured target; `sklearn.covariance.LedoitWolf` in the sidecar is fine — scikit-learn is BSD, off the AGPL path) or restrict to a factor-model covariance.
2. **Constrain first, optimize second.** Solve the Kelly allocation as a constrained problem (long-only, position caps, book cap) rather than clipping the unconstrained solution — clipping a `Σ⁻¹μ` solution after the fact destroys its optimality properties and can leave concentrated residual risk.

Pragmatic staging: start with **per-asset Kelly + caps + a driver-overlap penalty** (correct, simple, robust), and graduate to the constrained multi-asset optimizer only when the book holds enough simultaneous positions (>10) for covariance effects to matter.

### 14.3 The Math That Changes, Part 2: The Prior (the market is still a strong opponent)

Section 4.1's core discipline — the market's estimate is the prior; the model must earn deviation from it — has an exact equity analog, and skipping it is the most common way quant retail loses: raw model μ's are wildly overconfident.

**What is the "market price" of an expected return?** Not zero. Under equilibrium (CAPM-style reverse optimization), the market-implied excess return of an asset is

```
μ_prior = β_asset · ERP        (ERP = equity risk premium, ~4–5%/yr; β from regression vs. benchmark)
```

For crypto, where β against equities is unstable and there is no defensible equilibrium model, use the humbler prior `μ_prior = 0` (no alpha) and let the model fight uphill from there.

**Shrinkage, exactly parallel to 4.1:**

```
μ_final = λ_a · μ_model + (1 − λ_a) · μ_prior
```

with `λ_a` starting at **0.20** (slightly harsher than the prediction-market λ = 0.25, because return forecasts are noisier than event-probability forecasts) and re-fit from the asset ledger by minimizing out-of-sample forecast error. Volatility needs no shrinkage debate: σ is well-estimated from data — use realized vol (EWMA, ~60-day half-life), upgraded to implied vol where options exist. **Forecast the mean humbly and the variance empirically** — that asymmetry (μ is nearly unknowable, σ is measurable) should be visible everywhere in the implementation.

**The full Black–Litterman machinery** — prior `Π = δΣw_mkt`, views blended via `μ_BL = [(τΣ)⁻¹ + PᵀΩ⁻¹P]⁻¹[(τΣ)⁻¹Π + PᵀΩ⁻¹Q]` — is the multi-asset version of this same idea, with agent confidences mapping naturally onto the view-uncertainty matrix Ω. It is the *destination*, not the starting point: implement and validate the scalar shrinkage first, because BL with garbage Ω is just confident garbage with more linear algebra. Treat BL as a Phase 9+ upgrade gated on the scalar version demonstrating positive λ_a in re-fits.

### 14.4 Agent Roles Revert to Native Mode

The Section 5 adaptation layer becomes bidirectional. A second output contract joins `AgentSignal`:

```python
class AssetSignal(BaseModel):
    agent: str
    ticker: str
    horizon_days: int                    # forecast horizon (21 = ~1 month default)
    expected_excess_return: float | None # annualized; None = no opinion
    return_vol: float                    # annualized σ for the forecast distribution
    confidence: float                    # [0,1]; maps to view weight / Ω
    rationale: str
    inputs_used: list[DataRef]           # same staleness machinery as Section 5
```

Per-agent native roles (mostly *removing* the prediction-market adaptations): **Valuation** returns to being the anchor — DCF fair-value gap converted to expected return over the horizon with a mean-reversion speed assumption, stated explicitly in the rationale. **Fundamentals** scores map to historical forward-return spreads by score decile (base rates from data, not vibes). **Technical** supplies the σ estimate plus modest momentum/reversion tilts to μ — bounded so technicals can tilt but never dominate the valuation anchor. **Macro** conditions everything (regime-dependent shrinkage: in high-VIX regimes, λ_a drops further). **Sentiment/News** contribute short-horizon tilts and catalyst flags (earnings dates = the equity analog of resolution dates). **Risk Manager and Portfolio Manager** keep their Section 5 demotions: deterministic Rust code sizes positions; agents advise and veto. Aggregation pools `expected_excess_return` with confidence×routing weights — arithmetic pooling is correct here (returns are additive; no logit transform, which applies only to probabilities).

For crypto specifically, be honest in the routing table: **Valuation and Fundamentals return `None`** (there are no cash flows to discount). Crypto forecasts rest on Technical (vol + momentum), Macro (liquidity/rates regime), Sentiment, and News — which is to say they rest on less, which is to say λ_a for crypto should be lower still (start 0.10) and the gate (14.6) applies per asset class, not globally. If crypto forecasts never pass their gate, the app trades equities and *holds* crypto analysis at research-grade. That outcome is acceptable and likely.

### 14.5 Cost Models per Venue (the 5¢ rule, translated)

| Venue | Round-trip cost, realistic | Hurdle rule |
|---|---|---|
| Liquid US equities (Alpaca, zero commission) | 2–10 bps (spread + slippage); SEC/TAF fees negligible | Expected edge over horizon ≥ **3×** round-trip cost |
| Small-cap / illiquid equities | 20–100+ bps | Excluded from the universe initially (min ADV filter, e.g. $5M) |
| Crypto (exchange taker fees) | 10–60 bps **per side** + spread → 25–150 bps round trip | Expected edge ≥ 3× round trip → in practice ≥ **1–4%** expected move; short-horizon crypto signals below this are noise-trading with extra steps |

Two consequences worth stating plainly: (1) crypto's cost floor is 10–30× equities', so the crypto book will and should trade far less often; (2) the hurdle uses the *horizon-scaled* edge — 5%/yr of expected alpha is ~40 bps over a 1-month horizon, which barely clears the equity hurdle and fails the crypto hurdle entirely. The engine must do this horizon scaling explicitly (`edge_h = μ_final · h/365`), because annualized numbers make every trade look better than it is.

### 14.6 Validation: What Replaces the Brier Score

Brier scoring requires a resolved binary outcome; continuous returns need different instruments, and using the wrong ones (or none) is how a bad forecasting system survives long enough to lose real money:

1. **Information Coefficient (IC):** each forecast day, rank the universe by `μ_final` and compute the Spearman correlation with subsequent realized returns over the horizon. Grinold's fundamental law calibrates expectations: `IR ≈ IC·√breadth` — a sustained IC of 0.03–0.05 across a 100-name universe is *good* by institutional standards. Anyone expecting IC 0.3 has a bug or a bias.
2. **PIT (probability integral transform) — the reliability diagram's analog:** each forecast implies a distribution `F` (mean `μ_final·h`, vol `σ·√h`). At resolution compute `u = F(realized return)`. If forecasts are calibrated, the `u` values are Uniform(0,1); a PIT histogram that humps in the middle means σ is overestimated, U-shapes mean σ is underestimated (tails are fatter than modeled — expect exactly this for crypto; fix with a t-distribution, ν≈4, before trusting any crypto sizing, since Kelly with an underestimated σ² systematically oversizes).
3. **CRPS** as the single scalar for comparing forecast versions in re-fits, and paper P&L after modeled costs vs. a buy-and-hold benchmark as the bottom line.

**The gate, in code (per asset class):** live execution requires ≥ **60 forecast days × ≥ 50 names** (equities; ≥ 15 names for crypto) with (a) rolling IC t-statistic ≥ 2 — computed with Newey–West/HAC standard errors, because overlapping-horizon forecasts autocorrelate and naive t-stats overstate significance, this being precisely the kind of subtle math error the "our math must be correct" mandate exists for — (b) PIT histogram passing a uniformity check (Kolmogorov–Smirnov p > 0.05), and (c) paper P&L > costs. Cross-sectional breadth means this gate can be reached in ~3–4 calendar months of daily auto-forecasts, versus the sequential grind of the binary gate.

### 14.7 Execution Roadmap

**Alpaca first, for everything.** One official, well-documented REST API covering US equities *and* the major crypto pairs, with a first-class paper-trading environment that mirrors production — a perfect structural match for the gate philosophy (paper and live differ by a base URL and key, so Phase 8's paper book exercises the identical code path Phase 9 goes live on). Implemented in Rust in the core (the `apca` crate or direct `reqwest` against the REST API — it's clean HMAC-less bearer auth). Direct exchange integration (Coinbase Advanced Trade / Kraken, plain HMAC REST, also Rust) is a Phase 9+ option pursued only if Alpaca's crypto coverage or fees become binding; ccxt is noted and rejected for the money path (it's Python — execution stays in Rust, and the sidecar keeps its no-money invariant).

Order flow reuses the Phase 5 machinery verbatim: preview card (with the μ/σ/λ/hurdle math that justified the size and which cap bound it), explicit confirm, re-quote check, ledger write, notifications, circuit breakers. New execution-specific safeguards: limit orders only (no market orders in crypto, ever — thin books make market orders a slippage donation), a first-month size ramp at 10% of computed sizes, and order-value reconciliation against the ledger before submit.

### 14.8 Unified Risk Across Books

Full covariance unification between binary contracts and continuous assets is a research problem, not an engineering task — don't fake it. The staged design that is *correct*, just conservative:

1. **Top-level allocation:** bankroll splits into a prediction-market book and an asset book (default 50/50, user-configurable). Each book runs its own Kelly internally; neither can draw on the other.
2. **Cross-book driver caps (the load-bearing piece):** the Section 6.3 driver tags span both books. Long "Fed cuts in September" on Kalshi + overweight long-duration tech in the asset book + long BTC (a rates-sensitive asset) are *one* rates bet wearing three costumes. Net driver exposure is computed across books — binaries at stake value, assets at position value × driver beta — and capped globally (default 15% of total bankroll per driver).
3. **Shared circuit breakers:** total-bankroll drawdown triggers (Section 6.4) evaluate across both books; a blown-up asset book halts new Kalshi orders too, because the failure mode being guarded (the model family is miscalibrated, or the operator is tilting) is shared.

### 14.9 Phases 7–9 (sequenced after Phase 5; timeline indicative)

**Phase 7 — Asset forecast pipeline (~4 weeks).** `AssetSignal` contract + native-mode agent routing in the sidecar; `asset_forecasts` ledger table (forecast μ/σ/horizon, per-agent breakdown, resolution columns for realized return, PIT u, per-forecast costs); daily auto-forecast job over a starting universe (S&P 100 + top 15 crypto by volume, ADV-filtered); Asset Board UI (μ_final vs. hurdle, PIT/IC status per name). *Exit: ≥50 names forecast daily with full attribution; realized returns resolving into the ledger automatically.*

**Phase 8 — Calibration and sizing (~4 weeks + gate wait).** Scalar shrinkage with per-class λ_a; EWMA σ (t-distributed for crypto); quarter-Kelly + caps in Rust with the 14.2 guards property-tested (including "quarter-Kelly of the BTC example = 14%" as a literal golden test); cost hurdles with horizon scaling; paper book on Alpaca's paper environment; IC/PIT/CRPS dashboard alongside the Brier dashboard. *Exit: the 14.6 gate is enforced in code per asset class; monthly re-fit job extended to λ_a and agent weights.*

**Phase 9 — Live execution (~3 weeks, gated).** Alpaca live keys behind the gate flag; limit-order flow with preview/confirm/re-quote; 10% size ramp month; cross-book driver caps and shared breakers live. *Exit: a live equity order round-trips end-to-end; every 6.5 invariant re-verified against the asset book; crypto enabled only when its own gate passes.*

**Explicitly out of scope for this roadmap** (each a deliberate decision, revisit only with the ledger's evidence in hand): shorting, margin/leverage, options, crypto perpetuals and funding strategies, intraday/HFT horizons (the whole stack is built for daily-to-monthly horizons; sub-daily signals lose to costs per 14.5), and full Black–Litterman (gated per 14.3).

---

## Appendix A: Endpoint Reference

Trimmed to what the phases actually build; v1.0's speculative endpoints (discovery per asset class, watchlist CRUD, comparison suite, research/papers) move behind their backlog items.

```
# Health & meta
GET  /api/v1/health
GET  /api/v1/version                          # git SHA (license traceability)

# Market data (Phase 1)
GET  /api/v1/market/tracker
GET  /api/v1/market/tracker/{category}        # stocks|forex|commodities|bonds|etfs|crypto
GET  /api/v1/market/price/{ticker}
GET  /api/v1/market/history/{ticker}?period=&interval=
GET  /api/v1/market/search?q=

# Economic (Phase 2 — promoted from v1.0's Phase 5)
GET  /api/v1/economic/{country}/{indicator}   # gdp|cpi|unemployment|yield-curve|money-supply|policy-rate
GET  /api/v1/economic/macro/snapshot
GET  /api/v1/economic/releases/calendar       # NEW: powers cache-busting + catalyst detection

# Agents (Phase 2)
POST /api/v1/agents/market-opinion            # the primary endpoint (Section 7, Phase 2)
POST /api/v1/agents/{agent}                   # individual runs for the breakdown drawer / debugging

# Stock research (Phase 6)
GET  /api/v1/stock/{ticker}/info|technicals|fundamentals|quant|news

# Portfolio analytics (Phase 4)
POST /api/v1/portfolio/analyze                # positions in → empyrical metrics out

# Sentiment (backlog)
POST /api/v1/sentiment/text

# Asset forecasting (Section 14, Phases 7-9)
POST /api/v1/agents/asset-opinion             # ticker + horizon in → AssetSignal list out
POST /api/v1/agents/asset-universe            # batch daily auto-forecast run
GET  /api/v1/market/covariance?tickers=       # Ledoit-Wolf shrunk covariance (sklearn, BSD)
```

## Appendix B: Data Source Triage

v1.0 celebrated "16/16 data sources (100% coverage)." Coverage is not the goal; edge per unit of complexity is. The honest ledger:

| Source | Verdict | Reason |
|---|---|---|
| yfinance | **Core** | Underlies market data + Technical agent (note: unofficial API — see risk table) |
| EconDB | **Core** | Ground truth for Kalshi economic contracts — the single highest-value source |
| GNews / RSS | **High** | Catalysts and resolution-relevant news |
| Financial Datasets API | **High** | Agent inputs for company-event markets |
| Gemini / OpenAI / Ollama | **High** | Agent inference + cost routing |
| empyrical | **High** | Portfolio metrics (Apache-2.0 — permissive path) |
| FinanceDatabase | Medium | Discovery; Phase 6 |
| HuggingFace RoBERTa | Backlog | 2 GB for an unvalidated signal; opt-in download if sentiment earns weight |
| YouTube + transcripts | Backlog | Weakest signal-to-effort in the stack |
| Google Scholar | Backlog | No path to Kalshi edge |
| DataGovIN | Backlog | No liquid markets resolve on it |
| Fyers API | Backlog | Wrong geography for this product |
| Fincept Community API | Backlog | Not a trading capability |

**10 of 16 in active scope; 6 deliberately parked — and that's a feature of the plan, not a gap in it.**

## Appendix C: Kalshi Fee and Kelly Reference Math

**Fees.** Kalshi's trading fee per contract ≈ `0.07 · P · (1−P)` dollars (P = price in dollars), rounded up to the cent, charged on taker executions; settlement is free on most markets. Max 1.75¢/contract at P = 0.50. Verify the current schedule at implementation time — this constant is config, not code.

**Kelly for a binary.** Buying YES at effective cost `c` per $1 payout: win nets `(1−c)/c` per dollar staked with probability `p`; lose the stake with probability `1−p`. Kelly fraction `f* = (p·b − (1−p))/b` with `b = (1−c)/c` simplifies to:

```
f* = (p − c) / (1 − c)
```

Sanity checks: `p = c` → 0 (no edge, no bet); `p = 1` → 1 (certainty, full bankroll — and exactly why raw Kelly is never used); the system's quarter-Kelly + caps (Section 6) sit on top.

**Log-odds blending.** `logit(p) = ln(p/(1−p))`; blend in logit space; `p = 1/(1+e^{−logit})`. Clamp inputs to [0.01, 0.99] before logit to keep any single overconfident agent from dominating the pool.

**Continuous Kelly (Section 14) — derivation.** For an asset with excess-return mean `μ` and variance `σ²` per period, fraction `f` invested gives expected log-growth `g(f) = r + fμ − f²σ²/2` (Itô correction on the compounded return). Setting `g′(f) = μ − fσ² = 0` gives `f* = μ/σ²`, with maximal excess growth `g(f*) − r = μ²/(2σ²)`. Fractional-Kelly growth capture, exact: at `f = c·f*`, excess growth is `(2c − c²)·μ²/(2σ²)` — so half-Kelly (c = ½) captures `2(½)−(½)² = ¾` and quarter-Kelly (c = ¼) captures `2(¼)−(¼)² = 7/16 ≈ 44%`. Sanity checks: `c = 1` → 100%; `c = 2` → `4−4 = 0` (double-Kelly has zero excess growth, mirroring the binary case).

**Multi-asset Kelly.** Unconstrained optimum `f⃗* = Σ⁻¹μ⃗`; in practice solve `max f⃗ᵀμ⃗ − ½f⃗ᵀΣf⃗` subject to long-only, per-position, and book-total constraints, with Σ Ledoit–Wolf-shrunk (Section 14.2 guards). Never clip the unconstrained solution.

**Horizon scaling for the cost hurdle.** `edge_h = μ_final · h/365` versus round-trip cost; using annualized μ against per-trade costs overstates every edge by `365/h`.

**PIT calibration check.** Forecast distribution `F` (mean `μ_final·h/365`, sd `σ·√(h/365)`; Student-t ν≈4 for crypto); at resolution `u = F(r_realized)`. Calibrated ⇒ `u ~ U(0,1)`; hump-shaped PIT histogram ⇒ σ overestimated, U-shaped ⇒ σ underestimated (dangerous: Kelly oversizes when σ² is understated).

**IC significance.** Daily cross-sectional Spearman IC between forecast ranks and realized-return ranks; test `mean(IC) = 0` with Newey–West standard errors (lag ≥ horizon length) because overlapping horizons autocorrelate the IC series and naive t-stats overstate significance. Expectation-setting: `IR ≈ IC·√breadth` (Grinold); IC 0.03–0.05 sustained is institutionally good.

## Appendix D: Changes from v1.0

| Area | v1.0 | v2.0 |
|---|---|---|
| Goal framing | Integrate all 16 Fincept sources; "100% coverage" | One question: is this contract mispriced and by how much; 6 sources deliberately parked |
| Fincept target | Described v1.0.7 Python TUI | Corrected to v4 (C++/Qt6 + embedded Python); extraction target = the Python analysis layer, with a timeboxed spike |
| Architecture | Sidecar asserted | Sidecar *decided* against 4 alternatives; PyO3 rejected on license + GIL grounds; transport swappable |
| Licensing | Not mentioned | Full AGPL-3.0 strategy: two-repo boundary, contamination guards, permissive-path plumbing, escape hatches |
| Edge logic | "Market 72%, model 84%, bet the 12%" | Log-odds shrinkage toward market (λ = 0.25), fee + spread cost model, 5¢ threshold, failure-mode checklist; worked example shows the same trade correctly PASSing |
| Agents | One-line table rows | Per-agent prediction-market adaptation, routing weights by category, deterministic Rust aggregation with attribution; Portfolio Manager demoted from decider to reviewer |
| Risk | Multiplied ad-hoc Kelly factors | Quarter-Kelly + named hard caps, shared-driver correlation, circuit breakers incl. calibration-degradation auto-demotion, tested money invariants |
| Sequencing | 7 phases / 24 wks; execution last; macro in Phase 5 | Phase 0 (fix dead code) first; EconDB promoted to Phase 2; calibration gate (Phase 3) blocks execution (Phase 5); ~18 wks + honest backlog |
| Validation | None | Forecast ledger from week 1, paper trading, Brier-vs-market gate enforced in code, quarterly re-fit |
| Honesty items | Implied Fincept helps props | Stated: Fincept has no sports data; props deferred to a separate decision; open questions listed |

**v2.1 addendum (this version):** added Section 14 — the stocks-and-crypto expansion roadmap (Phases 7–9) that is the strategic point of the Fincept integration. Prediction markets are framed as the beachhead where the pipeline earns trust fast; the expansion reuses every layer (sidecar data, agents in native mode, shrinkage-toward-prior, cost hurdles, fractional Kelly, calibration gates, Rust-only money path) with the math translated correctly for continuous payoffs: binary Kelly → `μ/σ²` with exact fractional-Kelly growth capture, log-odds shrinkage → shrinkage toward β·ERP (equities) / zero-alpha (crypto) with Black–Litterman as the gated multi-asset upgrade, Brier/reliability → IC (Newey–West-tested), PIT, and CRPS, and the 5¢ rule → horizon-scaled hurdles of 3× round-trip costs per venue. Appendix C gains the corresponding derivations; Alpaca replaces the Robinhood scaffold as the execution target.

---

*End of document. The strategy in one sentence: fix what's broken first, keep the AGPL code behind a process boundary you'd want anyway, make the market's estimate your prior rather than your victim — whether that estimate is a contract price or an equilibrium return — and don't let the system touch real money in any asset class until the ledger proves it deserves to.*
