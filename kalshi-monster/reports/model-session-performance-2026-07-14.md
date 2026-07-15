# Model Session Performance Review — 2026-07-14

**Scope:** Latest Analyst sessions (esp. Jul 10–14), `predictions.db` track record, forecast ledger, and context pipeline.  
**Constraint:** Improvements must **not** alter Kelly / EV / edge / coerce math — only context, prompts, extraction, and policy rails.

---

## Overall grade

| Area | Grade | Notes |
|------|-------|-------|
| **Analyst LLM (deepseek-v4-flash-free)** | **D+** | Delivers tickets sometimes, but floods monologue, confuses units, overfits longshots, invents placeholders |
| **Edge engine / forecast ledger** | **B** | Correctly PASSed 35/35 live forecasts (conservative); agents mostly empty |
| **Graded prediction track record** | **F / N/A** | 15 predictions, **1 resolved = Loss**, 14 Pending; no meaningful Brier/CLV |
| **Context injection (pre-fix)** | **C−** | Real tape present, but price labels mixed ask$ with mid% → model derailed |
| **Decision extraction / rails (pre-fix)** | **C** | Unit coerce + settlement gates solid; no placeholder / spread / longshot rails |
| **System as trading aid** | **C−** | Usable scaffolding; free model + thin feedback loop undermines predictions |

**Composite model performance: D / C−** — infrastructure is ahead of the free model’s behavior; graded outcomes are not yet sufficient to claim edge.

---

## Data inventory

| Source | Count | Signal |
|--------|------:|--------|
| Chat sessions (meta) | 39 | Many empty shells |
| Sessions with messages | 14 | 5× deepseek free, 8× owl-alpha, 1× nemotron free |
| Latest session | `0d487d23…` Jul 14 01:10 | “Most mispriced today?” + “resend” |
| `predictions` rows | 15 | 14 Pending, **1 Loss** |
| `forecasts` rows | 35 | **All `pass`**, 0 resolved |
| `paper_lots` | 0 | No paper path exercised |
| `ml_predictions` | 0 | Unused |
| Market entry-price backtest | 1008 markets | Market calibration benchmark only (not app LLM) |

---

## Latest session deep-dive (`Predictions Jul 14 01:10`)

**User:** “What are the most mispriced markets on Kalshi today?”  
**Model:** `deepseek-v4-flash-free`  
**Tokens:** ~13k recorded (first assistant blob alone ~49k chars of monologue)

### What went wrong

1. **Leaked chain-of-thought** — Reply starts with `Thinking. 1. Analyze the Request…` and spends most of the budget debating data quality.
2. **Price label confusion** — Injected tape printed `Yes: $0.9400 (48.00%)` (ask dollars next to mid percent). Model correctly flagged contradiction but burned tens of thousands of characters on it.
3. **Multiple JSON revisions** — Four decision blocks for the same POR-16 NO ticket with slight fair drift (43.5 → 44.0).
4. **User forced resend** — “resend the final response” produced a *reconstruction monologue* instead of replaying the ticket.
5. **Weak fair-value basis** — Summer League cover rate “~56%” vs 66¢ is narrative, not tape-grounded evidence; confidence still **High**.

### What went right

- Eventually produced a structured `KalshiTradeDecision` JSON.
- Preferred a tight 1¢ book (POR-16) over 92¢ tennis books.
- Self-flagged unexecutable tennis markets as PASS.

**Session grade: D**

---

## Recent session pattern (Jul 10)

| Session | Failure mode | Example |
|---------|--------------|---------|
| 01:23 | Placeholder ticker stored | `KXEVENT-TICKER` prediction row |
| 01:33 | Extreme longshot overconfidence | SHIL YES @ 0.1¢ with fair **40%**, High confidence |
| 10:02 | Sub-1% TAKE with 4–5× “fair” | ESLO YES mkt 0.45 fair 2.0 |
| 10:02 follow-up | Readable summary OK-ish | Still re-emitted longshot TAKE |

Edge engine on the same day: **all PASS** (shrinkage + min_edge) — the hard path was more disciplined than free-model chat.

**Only graded Kalshi-style pick:** US–Iran agreement NO (TAKE Medium) → **Loss**.

---

## Root causes (ranked)

1. **Context formatting bug** — `Yes: $ask (mid%)` for wide books looks like corrupt data.
2. **Free thinking models dump monologue into content** — coalesce promotes reasoning; no deliverable strip.
3. **Schema example ticker `KXEVENT-TICKER`** — model copy-pastes it into live tickets.
4. **No post-LLM quality rails** for spread>edge, longshot multiplies, or placeholder ids.
5. **Retrieval still surfaces thin/wide books** for generic “mispriced” scans.
6. **Almost no graded flywheel** — cannot calibrate LLM fairs without resolved outcomes.
7. **Resend/summarize follow-ups** re-analyze instead of replaying.

---

## Improvement plan (no math changes) — implementation status

| # | Change | Status |
|---|--------|--------|
| 1 | Format retrieved markets as **bid / ask / mid $ + mid-implied %**; flag WIDE_SPREAD / THIN | ✅ Done |
| 2 | Prefer tight-spread + non-zero volume in retrieval scoring | ✅ Done |
| 3 | Prompt: JSON-first, no Thinking monologue; calibration judgment; never placeholder tickers | ✅ Done |
| 4 | `prefer_deliverable_content` strip monologue before store/return | ✅ Done |
| 5 | Resend/summarize follow-up instruction | ✅ Done |
| 6 | Prefer **last** valid JSON decision; reject `KXEVENT-TICKER` | ✅ Done |
| 7 | `enforce_prediction_quality_rails` (spread>edge → PASS; longshot× without Live → WATCH; placeholders) | ✅ Done |
| 8 | Paper path re-applies quality rails | ✅ Done |
| 9 | Reasoning `exclude: true` on OpenRouter reasoning-capable models | ✅ Done |
| 10 | Unit tests for all of the above (60 chat tests pass) | ✅ Done |

### Follow-ups (not in this patch; still no math)

- **Accumulate resolved forecasts** and inject a short calibration card into the prompt once n≥30.
- **Default model guidance:** free DeepSeek is fine for smoke tests; use a stronger non-free model for real TAKE decisions.
- **Auto-grade path** for Kalshi settlements → close the 14 Pending rows.
- **Agent fill rates** (technical/news/macro currently null) — better context, still not Kelly math.
- **UI:** hide streamed monologue live if gateway still emits reasoning before content.

---

## What was explicitly *not* changed

- Kelly formula, EV ROI, edge points derivation  
- Shrinkage λ / min_edge / fee_multiplier math  
- Probability coerce / price coerce formulas (only call sites + rails)  
- Isotonic calibrator fitting  

Rails only **demote decision / confidence / stake** when policy fails; they never rewrite `fair_probability_pct` or recompute Kelly coefficients differently.

---

## Expected impact

| Symptom | Before | After (expected) |
|---------|--------|------------------|
| Ask$ vs mid% confusion | Frequent | Eliminated by label fix |
| 30–50k Thinking dumps stored | Common on free DeepSeek | Stripped to JSON+summary |
| `KXEVENT-TICKER` predictions | Observed | Rejected at extract + paper |
| Longshot fair 40% on 0.1¢ | TAKE High | WATCH/PASS under rails |
| Wide illiquid TAKE | Possible | Forced PASS |
| Graded skill | Unknown / 0–1 | Still needs resolution flywheel |

---

## Bottom line

The free model’s **process quality** was the bottleneck more than the app’s math. Context labels and monologue made it look incompetent even when it eventually found a tight book. Quality rails and deliverable-first output should raise Analyst reliability from **D+ toward B−** on process metrics; **true prediction skill still requires graded settlements** and (ideally) a stronger model for live TAKE calls.
