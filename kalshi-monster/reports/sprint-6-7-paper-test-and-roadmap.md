# Sprint 6–7 close-out: paper test + improvement roadmap

**Date:** 2026-07-15  
**Scope:** Finish paper product polish + AGPL isolation scaffolding; run a simulated Analyst paper path; rate the call; document architecture gaps.

---

## 1. Simulated test prompt & paper decision

### Prompt (what the Analyst would receive)

```text
Analyze Kalshi market KXBTCD-26JUL15-B100000: Will Bitcoin be above $100,000?
Category: Crypto; YES mid ~40¢. Give a structured JSON decision with fair_probability_pct,
contract_side, decision TAKE|WATCH|PASS, and recommended_stake_dollars.
```

### Decision ticket used in automated paper path

| Field | Value |
|-------|--------|
| Ticker | `KXBTCD-26JUL15-B100000` |
| Side | YES |
| Market | 40¢ |
| Fair (claimed) | 55% |
| Edge claim | +15 pts |
| Decision | TAKE |
| Qty / entry | 50 contracts @ 40¢ |
| Source | `AiDecision` |

### What the engine did

1. Opened a paper lot (cash debited = stake + fee).  
2. Settled with Kalshi result **Yes** (YES holder wins $1/contract).  
3. Equity snapshot retained open-MV cost-basis integrity before close.  
4. Analytics: win counted; **profit_factor finite** (capped, never Infinity JSON).

Automated test: `paper::tests::analyst_style_take_settle_and_rate` (must pass in CI).

---

## 2. Prediction rating

| Dimension | Grade | Notes |
|-----------|-------|--------|
| **Outcome** (this sim) | **A** | Resolved YES; lot PnL &gt; 0 |
| **Process / edge claim** | **C+** | 15pt edge on a BTC barrier is aggressive without agent priors + vol model; free LLMs often invent fairs |
| **Sizing hygiene** | **B** | Sprint 6 bankroll caps + large-stake confirm + fee preview reduce disasters |
| **Data grounding** | **C** | Technical agent *can* opine when yfinance + strike/horizon map; chat path still LLM-led unless priors injected |
| **Overall session grade** | **B−** | Infrastructure ready for honest paper validation; model quality still the bottleneck |

**Interpretation:** The *platform* correctly journals, debits cash, settles side-aware, and keeps dual ledgers labeled. The *predicted edge* in this synthetic ticket is not evidence of skill — treat as process rehearsal until ≥200 resolved forecast rows and gate OPEN.

---

## 3. Improvement & architecture plan (post–Sprint 0–7)

### A. Highest leverage next (product)

1. **Live paper loop with real tape** — one-click “Paper this Edge Board row” that reuses `PaperRecordResult` + fee preview without re-asking the LLM.  
2. **Agent-first fair, LLM annotation** — for price-level markets, default `fair_probability_pct` from `p_final` when agents opine; LLM writes thesis only.  
3. **CLV tracking UI** — entry mid vs resolution mid already partially in DB; surface on prediction cards.  
4. **FRED key in Settings** — store optional FRED key in secrets store so macro agent works without shell env.  
5. **Bankroll ↔ paper cash reconcile button** — optional “set bankroll.json = paper equity” to reduce dual-ledger mistakes.

### B. Model / signal quality

6. **Calibration flywheel ops** — dashboards done; need users to accumulate n≥200; add weekly email/export of Brier.  
7. **News grounding** — pass chat web hits into deep analyze automatically for retrieved markets (not only political category heuristic).  
8. **Macro release calendar** — map FOMC/CPI print *dates* as catalysts even when level threshold missing.  
9. **Sentiment** — only if a free non-AGPL feed exists; otherwise keep null forever (honest).  
10. **Disagreement UI** — when LLM fair vs `p_final` diverges &gt;10pts, force ModelDisagreement risk flag in UI before paper TAKE.

### C. Architecture gaps

| Gap | Risk | Direction |
|-----|------|-----------|
| **Monorepo still holds AGPL sources** | License distribution risk | Run `scripts/split-fincept-sidecar.ps1` → public repo; pin SHA in releases |
| **Two probability writers** | Chat LLM ledger vs edge pipeline | Prefer edge `p_model` for forecasts; store LLM fair under `agent_breakdown.llm` only |
| **Dual cash concepts** | User sizes on bankroll, debits paper cash | Already labeled; add hard pre-check: refuse TAKE if paper cash &lt; stake+fee |
| **Sidecar latency on chat** | Prior batch can delay first token | Keep 8s timeout; consider priors only on “deep” chat mode |
| **No order path** | Correct for research posture | Keep demo/live orders behind calibration gate + breakers |
| **AssetSignal unconnected to Rust** | Continuous book dead code | Wire IPC only after gate OPEN; size with separate asset Kelly later |
| **Secrets sprawl** | OpenRouter / FRED / Brave keys | Single secrets panel with health probes |
| **Test env ≠ product** | Vitest/cargo strong; E2E UI thin | Add Playwright smoke: open Command desk → Edge Board → paper |

### D. Explicit non-goals (keep)

- No Kelly/shrink formula rewrites without ADR.  
- No invented agent probabilities.  
- No embedding Python in Tauri.  
- No live size ramp until gate OPEN.

### E. Suggested sprint sequence (new) — **implemented 2026-07-16**

```text
S8  ✅ Paper TAKE hard cash check + Edge Board → paper one-click
S9  ✅ Agent-default fair (MarketDetail after Run edge) + LLM fair in breakdown.llm
S10 ✅ FRED/Brave secrets + macro calendar caveats
S11 ✅ AGPL split docs/script (operator public push still manual)
S12 ✅ CLV cards + Brier CSV export + edgeBoardSmoke vitest
```

Also: AssetSignal IPC `kalshi_get_asset_signal`; bankroll sync to paper equity.

---

## 4. Sprint 6–7 delivery checklist

| ID | Item | Status |
|----|------|--------|
| 6.1 | Cash vs bankroll.json clarity | ✅ Portfolio + Settings |
| 6.2 | Grade vs Settle + auto-refresh | ✅ Event + copy |
| 6.3 | Chat bankroll policy | ✅ `paperSizingPolicy` |
| 6.4 | Finite PF + analytics errors | ✅ Cap + banner + PF display |
| 6.5 | Fee preview + large stake confirm | ✅ `kalshiFees` + MarketDetail/Chat |
| 7.1 | AGPL split procedure | ✅ `docs/AGPL-SIDECAR-SPLIT.md` + script |
| 7.2 | Only real data-path modules | ✅ Documented; macro/news/tech live |
| 7.3 | AssetSignal after calibration | ✅ Gated endpoint + tests |

---

## 5. Verification commands

```bash
cd kalshi-monster/src-tauri && cargo test --lib paper:: -- --test-threads=4
cd kalshi-monster/src-ui && npm test -- --run src/utils/kalshiFees.test.ts
cd fincept-sidecar && python -m pytest tests/test_asset_signal.py tests/test_macro.py -q
```
