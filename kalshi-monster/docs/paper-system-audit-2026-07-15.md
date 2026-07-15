# Paper Trading System Audit — 2026-07-15

> **Forward work sequenced in:** **`docs/MASTER-PLAN.md`** (Sprint 0 = paper trust polish; Sprint 6 = paper product). This audit remains the deep dive on bugs and architecture.

## How the flow works

```
Analyst JSON / MarketDetailPanel decision
        │
        ▼
UI extractPaperDecision / buildDecision  →  kalshi_record_paper_decision IPC
        │
        ▼
Rust: sanitize_units → quality rails → settlement gate → bankroll/breaker/Kelly
        │
        ▼
paper::record_paper_decision (atomic txn):
  1. predictions row
  2. forecasts ledger row
  3. edge fields on prediction
  4. IF TAKE + stake: paper_lots + debit paper_account
        │
        ▼
paper_settle_pending (manual / future poller)
  fetch Kalshi result → close_lot → credit proceeds
```

**Key files**

| Layer | Path |
|-------|------|
| Engine | `src-tauri/src/paper/mod.rs` |
| IPC | `commands/kalshi_analysis.rs` (`kalshi_record_paper_decision`) |
| Commands | `paper_get_analytics`, `paper_get_positions`, `paper_settle_pending`, `paper_reset_account` |
| UI extract | `src-ui/src/utils/paperFromChat.ts` |
| UI entry | `ChatView.tsx`, `MarketDetailPanel.tsx` |
| Schema | `chat/decision_schema.rs` |

---

## Critical / high findings

### H1. NO-side settlement used YES exit price (inverted PnL) — **FIXED this pass**

`settle_pending` set exit to 100¢ only when result was Yes, for **all** lots.  
NO winners were paid **0**; NO losers were paid **100**.

**Fix:** `settlement_exit_cents_for_side(side, actual)` + tests.

### H2. IPC fragility (same class as `model_disagreement`) — **partially FIXED**

| Field | Issue | Status |
|-------|--------|--------|
| `model_disagreement` | required bool | `#[serde(default)]` + UI always set |
| `evidence`, `risk_flags` | missing arrays | `#[serde(default)]` |
| `data_quality` | `"ChatExtract"` not in enum | added `ChatExtract` + aliases |
| `risk_flags` unknown strings | hard fail | `#[serde(other)]` → `Other` |
| enum case | `"yes"` / `"take"` | aliases + SCREAMING_SNAKE for actions/sides |
| confidence / risk naming | space-stripped flags in MarketDetailPanel | still weak; Other absorbs |

### H3. WATCH opened cash positions — **FIXED this pass**

Lot opened when `contract_side != PASS` only. **WATCH YES** could debit cash.

**Fix:** require `decision == TAKE` **and** non-PASS side.

### H4. `entry_price` on prediction stored market % not entry $ — **FIXED**

```rust
// was: Some(decision.market_price_pct)  // e.g. 34.0 misread as $34
entry_price: Some(decision.price_to_enter)
```

### H5. Settlement not on auto-grade path — **FIXED (2026-07-15 follow-on)**

- `settle_pending` now syncs prediction Win/Loss + resolves forecasts
- Lots store `prediction_id` when opened via paper decision
- Auto-grade poller runs paper settle whenever open lots exist
- Manual **Grade pending** and **Resolve forecasts** also settle paper lots

### H6. Dual ledgers diverge

Chat extract → predictions/forecasts without lots still possible.  
Paper path now links lot → prediction_id and re-syncs on settle.

---

## Medium findings

| ID | Issue | Impact |
|----|--------|--------|
| M1 | No fee on settlement (entry fee only) | Slight optimistic PnL vs Kalshi |
| M2 | `close_time` on paper forecast often `now` not market close | Calibration timing wrong |
| M3 | MarketDetailPanel risk flags strip spaces → invalid enum names | Was IPC fail; now → Other |
| M4 | Equity snapshots not continuous (only on trade/close) | Drawdown charts sparse |
| M5 | No lot→prediction foreign key | Hard to join journal to chat |
| M6 | Paper settle does not update linked prediction outcome | Predictions tab stays Pending |
| M7 | Insufficient funds error opaque in UI | Need clearer “reduce stake / reset paper” |
| M8 | No auto-settle after grade or on dashboard load | Manual only |
| M9 | Bankroll summary merge of paper+kalshi bets is heuristic | Analytics can double-count semantics |
| M10 | Qty from stake/entry may be fractional | OK for paper; fees use float contracts |

---

## Low / product gaps

- No position scaling / partial close  
- No “paper only” forced mode UI beyond breaker flag  
- No per-lot fee column (fee baked into stake)  
- No CLV on paper lots  
- Portfolio view may not refresh after Chat record without navigation  
- No confirmation modal for large TAKE  
- Reset account is hard wipe — no soft archive  

---

## What already works well

- Atomic `record_paper_decision` txn (prediction + forecast + lot)  
- Fee on open via `order_fee`  
- Breaker stake scaling  
- Settlement gate force PASS on settled tape  
- Quality rails (spread > edge, longshot, placeholders)  
- Balance check + concurrent trade test  
- Starting $10k account bootstrap  

---

## Prioritized roadmap

| Priority | Work |
|----------|------|
| **P0** | ~~NO settlement fix~~, ~~TAKE-only lots~~, ~~entry_price~~, ~~serde defaults~~ |
| **P1** | Call `paper_settle_pending` from auto-grade / Calibration “Resolve” |
| **P1** | Sync prediction outcome when paper lot closes |
| **P1** | Integration test: record TAKE → settle Yes/No both sides |
| **P2** | Use real market close_time on paper forecasts |
| **P2** | UI: show opened lot id + cash after record; force settle button on Paper tab |
| **P2** | Harden MarketDetailPanel decision builder (model_disagreement, Pascal risk flags) |
| **P3** | Fee disclosure, CLV, continuous equity marks |

---

## Verification (this pass)

- Unit: `settlement_exit_cents_side_aware`, `no_side_settlement_pays_on_no_result`  
- Prior: `deserialize_omits_model_disagreement_defaults_false`  
- Manual: Record paper on Stevens ticket without `model_disagreement` field  

---

## Frontend-specific (from UI audit)

| Issue | Fix status |
|-------|------------|
| `ChatExtract` data_quality | Backend accepts + aliases |
| MarketDetailPanel risk flags (`Earlyclose` etc.) | **Fixed** → proper `RiskFlag` codes |
| Success copy always claims lot opened | **Fixed** — ChatView distinguishes TAKE lot vs journal-only |
| Chat doesn't pass bankroll policy into extract | Open (backend still caps) |
| Grade vs Settle dual buttons unclear | Open |
| Structured IPC response `{ lot_opened, final_decision }` | Open |

## Bottom line

Paper is a solid **skeleton** (account, lots, fees-on-open, atomic journal) with **dangerous settlement math on NO** and **IPC that assumes complete schema**. This pass fixes the NO inversion, TAKE-only lots, entry_price, IPC/serde landmines, market-panel risk flag codes, and truthful chat success copy. Remaining leverage is **auto-settle + prediction sync** so the journal is trustworthy without manual steps.
