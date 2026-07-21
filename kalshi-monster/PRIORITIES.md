# Kalshi Monster — Priority Roadmap

Last updated: 2026-07-21 (late afternoon cron — preferred-series tape + legacy B-leg gate filter)

## Maintenance notes (2026-07-21, late afternoon cron) — preferred-series snapshots + honest gate filter

- Health: `cargo check`, `tsc` clean, **308** lib tests (0 failed, 9 ignored); working tree dirty with coherent edge-integrity WIP → committed this pass
- Branch: `fix/edge-measurement-integrity`
- Auto-remediation: none (artifacts); committed coherent WIP (legacy B-leg filter + preferred tape + paper-lot opener)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+6** Jul-21 13:00Z BTC hourly outcomes (B65950/66350/66450/66550/66650/67150; 1 Yes B66550 / 5 No)
  - Ledger: **430** total / **352** resolved / **78** unresolved (after +24 pipeline writes this pass)
  - [raw] Brier p_final **0.1463** vs p_market **0.1461** vs p_model **0.2720**
- **Honest gate (post legacy B-leg filter):** **eligible = 19/200 (9.5%) — LOCKED**
  - Filter: pre-`2026-07-21T20:00Z` B-leg model rows priced as P(S>K) are excluded (28 legacy rows filtered; mirrors Rust `is_legacy_b_leg_model_row`)
  - Eligible Brier p_final **0.3186** ≤ p_market **0.3257** (still beats market on thin clean sample)
  - `paper_lots` = **0** — PnL leg unmet. `open_paper_lots_from_forecasts.py --dry-run` → **0 candidates** (post-geometry pipeline correctly emits `pass` only — no fee-aware edge above theta)
  - Do **not** flip live execution.
- **Model sample-build:** two `live_forecast_pipeline.py` runs wrote **+24** forecasts with `p_model` (KXBTC/KXETH/KXINX/KXWTI mix; all verdict=`pass`)
- **Shipped this pass:**
  1. **Preferred-series price snapshots (Rust):** `price_tracker::spawn_preferred_series_snapshot_task` + `KalshiClient::fetch_preferred_series_markets` (KXBTC/ETH/INX/NASDAQ/GOLD/WTI/AAPL/TSLA/NVDA) every 120s via Tauri runtime; wired in `lib.rs`
  2. **Pipeline tape warm:** `live_forecast_pipeline.py` writes `kalshi_price_snapshots` for every preferred market it fetches (**239** snaps this run — KXETH/INX/WTI/TSLA now have history; KXBTC timed out once on API)
  3. **Standalone** `scripts/snap_preferred_series.py` for app-closed cron path
  4. **Legacy B-leg gate filter** in Rust `calibration.rs` + Python `kalshi_ticker.py` + load path in `forecast.rs` (regression test + 93 Python ticker/paper tests green)
  5. **Paper lot opener** `scripts/open_paper_lots_from_forecasts.py` (dry-run default; execute when post-fix trade_* appear)
- **Autopsy note:** papered historical `trade_yes` still −0.30 on poisoned pre-fix book; fade-model counterfactual still looks good on that sample — do not promote until post-fix n grows
- **Next cron:** resolve ETH 17:00Z + INX H1600 when results print; re-run pipeline + snap; open paper lots only when verdict≠pass post-fix; leave app running for preferred-series snapshot loop
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 on **clean** sample + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — gate correctly LOCKED

## Maintenance notes (2026-07-21, Phase 0–2 implement) — **legacy filter + tape + paper path**

- Branch: `fix/edge-measurement-integrity`
- **Plan:** `.hermes/plans/2026-07-21_163822-kalshi-edge-capture-next-priorities.md` (Tasks 1–4)
- **Task 1 — legacy B-leg filter (gate honesty):**
  - Python `kalshi_ticker.is_legacy_b_leg_model_row` + `eligible_resolved_rows` skip pre-`2026-07-21T20:00Z` B-legs without `floor_strike`/`cap_strike` in breakdown
  - Rust `edge_engine::calibration::{is_legacy_b_leg_model_row, eligible_rows}` + loader carries ticker/created_at/breakdown
  - Eligible dropped **23 → 19/200** (poisoned B-leg model rows excluded) — correct
- **Task 2 — post-fix pipeline smoke:** +12 model rows; B-leg `p_model` now ≈ mkt (e.g. 0.108 vs 0.100) — **no more 0.99 vs 0.05**; all `pass` (no fake trade_yes); gate **LOCKED**
- **Task 3 — preferred-series tape:**
  - In-app: `price_tracker::spawn_preferred_series_snapshot_task` + `client.fetch_preferred_series_markets`
  - Cron: `scripts/snap_preferred_series.py` → **369** snaps this run (KXBTC/ETH 100 each, INX/NDX/WTI/TSLA live)
- **Task 4 — paper auto-lot path:** `scripts/open_paper_lots_from_forecasts.py` (+ tests). Dry-run **0 candidates** (honest post-fix book is all `pass` until real edge appears). `paper_lots` still 0 — PnL leg unmet by design
- **Tests:** Python ticker+paper **93** green; Rust calibration **22** green; preferred_series unit **1** green
- **Ops cadence:**
  1. `fincept-sidecar/.venv/Scripts/python.exe scripts/snap_preferred_series.py`
  2. `… resolve_settled_forecasts.py`
  3. `… live_forecast_pipeline.py`
  4. `… open_paper_lots_from_forecasts.py --execute` (when candidates > 0)
- **No live execution** — gate still LOCKED (eligible 19/200, paper PnL 0)

## Maintenance notes (2026-07-21, edge/calibration deep-dive) — **CRITICAL model geometry fix shipped**

- Branch: `fix/edge-measurement-integrity`
- **Root cause of YES bias / fake trade_yes:** technical agent priced Kalshi **B-legs as P(S>K)** but B-legs are **range/bracket** contracts (`floor < S < cap`). Live API confirmed (e.g. `KXBTC-…-B73250` = $73,200–73,299.99). On lower bins with spot above the bin this emitted tech≈0.99 vs mkt≈0.03 — the entire `trade_yes` 1–6 track record was poisoned by this.
- **Autopsy (live DB, fee-aware mid entry, $1 payout):** papered ledger `trade_yes` PnL **−0.026** (1W/6L); mean(p_model−y)=**+0.248**; λ≈0.20 slightly best on thin sample; high-mkt “edge” was mostly sports longshot tape not a real book.
- **Shipped:**
  1. `fincept-sidecar/agents/technical.py` — `infer_contract_spec` + `binary_bracket_prob` / above / below; API `floor_strike`/`cap_strike` preferred; ticker `-B#` → bracket, `-T#` → one-sided
  2. `scripts/live_forecast_pipeline.py` — pass real API geometry; **stop fabricating** `[mid]*5` tape; Rust-parity **trade_yes/trade_no** + min_confidence; rank near-spot brackets first; load real snapshots when present
  3. Tests: bracket≪call regression + geometry parsers — **21** fincept tests green
  4. `scripts/_edge_autopsy.py` — read-only counterfactual / bias / λ sweep on predictions.db
- **How to capitalize largest real edges next:**
  1. Re-run live pipeline under fincept venv → new `p_model` on near-ATM brackets + T-thresholds only
  2. Prefer **density near spot** (mids 8–40¢ brackets) and one-sided T legs; avoid lottery deep OTM bins
  3. Open **small paper lots** on post-fix `trade_yes`/`trade_no` (still 0 lots — PnL gate unmet)
  4. Point price_tracker at preferred series so contract_tape gets real mids (0 snaps on all prior trade_yes tickers)
  5. Do **not** live-trade until eligible≥200 + paper PnL>0
- **Ops:** `fincept-sidecar/.venv/Scripts/python.exe scripts/live_forecast_pipeline.py` (scrub Hermes PYTHONPATH)
- **No live execution flip** — gate still LOCKED

## Maintenance notes (2026-07-21, evening cron) — health green; **+12 resolves, +12 p_model, eligible 23/200**

- Health: `cargo check`, `tsc` clean, **306** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Branch: `fix/edge-measurement-integrity` (ahead of master with honest-calibration work)
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+12** outcomes at ~16:28Z
  - Jul-21 09:00Z BTC hourlies: KXBTC×5 (B66150–B66550; 1 Yes B66550 / 4 No) — all had p_model
  - ATP Gstaad BORLUZ/BUEPER: spreads×3 + totals×4 (mixed Y/N)
  - Progress raw: **346** resolved / **60** unresolved / **406** total (was 334/60/394 midday; +12 resolve then +12 pipeline)
  - [raw] Brier p_final **0.1466** vs p_market **0.1468** vs p_model **0.2649**
  - Note: midday trade_yes KXBTC-09 B66150/B66250 both resolved **No** (model overconfident Yes) — honest loss on model sample
- **Honest gate:** **eligible = 23/200 (11.5%) — LOCKED** (was 22 midday; +1 eligible from BTC 09:00Z model-bearing resolves)
  - Eligible Brier p_final **0.2943** ≤ p_market **0.2986** (still beats market on thin model sample)
  - `paper_lots` = **0** — PnL leg still unmet. Do **not** flip live execution.
- **Model sample-build:** `live_forecast_pipeline.py` wrote **12** forecasts with `p_model` set (technical + contract_tape)
  - Mix: KXBTC×6 Jul21 13:00Z (incl. 2× trade_yes B66350/B65950) + KXINX×6 (Jul21 H1600×3 + Jul24 H1600×3)
  - agents_opining=2 on all; unresolved **57/60** have p_model
  - Notable open trade_yes for paper leg (app UI):
    - KXBTC-26JUL2113-B65950 (final 0.313 vs mkt 0.140) close ~17:00Z
    - KXBTC-26JUL2113-B66350 (final 0.198 vs mkt 0.115) close ~17:00Z
    - KXINX-26JUL21H1600-B7487 (final 0.268 vs mkt 0.165) close ~20:00Z
    - KXETH-26JUL2117-B1830 / B1870; KXNASDAQ100-26JUL24H1600-T27600
- **Still open / next settles:** KXBTC Jul21 13:00Z (6), KXINX Jul21 H1600 (7), KXETH Jul21 17:00Z (5); no Jul21 city-high rows in ledger
- **Ops note:** run live pipeline with fincept venv:
  `fincept-sidecar/.venv/Scripts/python.exe scripts/live_forecast_pipeline.py`
- **Next cron:** re-resolve after Jul21 13:00Z BTC (~17:00Z) + INX 16:00Z print (~20:00Z) + ETH 17:00Z; re-run live pipeline; **operator: open paper lots** on high-conviction trade_yes in app UI so PnL leg can trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — full gate correctly LOCKED on eligible sample + paper PnL

## Maintenance notes (2026-07-21, midday cron) — health green; **+69 resolves, +12 p_model, eligible 22/200**

- Health: `cargo check`, `tsc` clean, **306** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Branch: `fix/edge-measurement-integrity` (ahead of master with honest-calibration work)
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+69** outcomes in one pass
  - Jul-20 17:00Z crypto: KXBTC×12 (all No) + KXETH×9 (1 Yes B1910 / 8 No)
  - Jul-20 city highs: CHI×6 + DEN×6 + LAX×5 + MIA×4 + NY×6 + PHIL×5
  - Jul-20 INX 16:00Z: **15** rows across B7362–B7537 (incl. multi-row B7462/7487/7512 with p_model)
  - +1 ITF match (BRADOM-DOM Yes)
  - Progress raw: **334** resolved / **60** unresolved / **394** total (was 265/117/382 evening; after pipeline)
  - [raw] Brier p_final **0.1451** vs p_market **0.1455** vs p_model **0.2519**
- **Honest gate:** **eligible = 22/200 (11.0%) — LOCKED** (was 21 evening; +1 eligible from INX/model-bearing resolves)
  - Eligible Brier p_final **0.3045** ≤ p_market **0.3101** (still beats market on thin model sample)
  - `paper_lots` = **0** — PnL leg still unmet. Do **not** flip live execution.
- **Model sample-build:** `live_forecast_pipeline.py` wrote **12** forecasts with `p_model` set (technical + contract_tape)
  - Mix: KXBTC×5 (incl. 2× trade_yes on Jul21 09:00Z B66150/B66250) + KXINX×7 (Jul21 H1600 + Jul24 H1600)
  - agents_opining=2 on all; unresolved now **50/60** have p_model (stronger sample-build mix)
  - Notable open trade_yes for paper leg (app UI): KXBTC-26JUL2109-B66150 (final 0.179 vs mkt 0.090), KXBTC-26JUL2109-B66250, prior KXETH-26JUL2117-B1830/B1870, KXNASDAQ100-26JUL24H1600-T27600
- **Ops note:** run live pipeline with fincept venv:
  `fincept-sidecar/.venv/Scripts/python.exe scripts/live_forecast_pipeline.py`
- **Next cron:** re-resolve after Jul21 09:00Z BTC hourlies (~13:00Z window) + Jul21 INX 16:00Z + overnight city highs; re-run live pipeline; **operator: open paper lots** on high-conviction trade_yes in app UI so PnL leg can trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — full gate correctly LOCKED on eligible sample + paper PnL

## Maintenance notes (2026-07-20, evening cron) — health green; **+5 resolves, +12 p_model, eligible 21/200**

- Health: `cargo check`, `tsc` clean, **306** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Branch: `fix/edge-measurement-integrity` (ahead of master with honest-calibration work)
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+5** Jul-20 13:00Z BTC hourly outcomes
  - KXBTC-26JUL2013 B65050/65250/65350/65450/65550 (1 Yes / 4 No)
  - Progress raw: **265** resolved / **117** unresolved / **382** total (was 260/110/370 midday; +12 model rows this pass)
  - [raw] Brier p_final **0.1574** vs p_market **0.1579** vs p_model **0.2873**
- **Honest gate:** **eligible = 21/200 (10.5%) — LOCKED** (was 20 midday; +1 eligible from BTC 13:00Z with p_model)
  - Eligible Brier p_final **0.3175** ≤ p_market **0.3239** (still beats market on thin model sample)
  - `paper_lots` = **0** — PnL leg still unmet. Do **not** flip live execution.
- **Still open / not yet printable:**
  - Jul-20 city highs (CHI/DEN/LAX/MIA/NY/PHIL) — still `status=active` at pass time (~20:47Z)
  - KXINX-26JUL20H1600-* — `status=closed` but `result=''` (cannot invent; re-poll next cron)
  - KXBTC/ETH-26JUL2017 hourlies — still active (likely ~21:00Z settle window)
- **Model sample-build:** `live_forecast_pipeline.py` wrote **12** forecasts with `p_model` set (technical + contract_tape)
  - Mix: KXETH×5 (incl. 2× trade_yes) + KXBTC×2 + KXINX×4 + KXNASDAQ100×1 (1× trade_yes); agents_opining=2 on all
  - Notable trade_yes for paper leg (app UI): KXETH-26JUL2117-B1830 (final 0.254 vs mkt 0.115), KXETH-26JUL2117-B1870, KXNASDAQ100-26JUL24H1600-T27600; prior INX B7462 still closed-no-result
- **Ops note:** run live pipeline with fincept venv:
  `fincept-sidecar/.venv/Scripts/python.exe scripts/live_forecast_pipeline.py`
- **Next cron:** re-resolve after 17:00Z crypto hourlies + INX result print + overnight city highs; re-run live pipeline; **operator: open paper lots** on high-conviction trade_yes in app UI so PnL leg can trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — full gate correctly LOCKED on eligible sample + paper PnL

## Maintenance notes (2026-07-20, midday cron) — health green; **+5 resolves, +12 p_model, cron PYTHONPATH fix**

- Health: `cargo check`, `tsc` clean, **306** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Branch: `fix/edge-measurement-integrity` (ahead of master with honest-calibration work)
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+5** Jul-20 09:00Z BTC hourly outcomes
  - KXBTC-26JUL2009 B64450/64550/64650/64750/64850 (1 Yes / 4 No)
  - Progress raw: **260** resolved / **110** unresolved / **370** total (was 255/103/358 morning)
  - [raw] Brier p_final **0.1575** vs p_market **0.1583** vs p_model **0.2633**
- **Honest gate:** **eligible = 20/200 (10.0%) — LOCKED** (was 19 morning; +1 eligible from BTC hourlies with p_model)
  - Eligible Brier p_final **0.3045** ≤ p_market **0.3112** (still beats market on thin model sample)
  - `paper_lots` = **0** — PnL leg still unmet. Do **not** flip live execution.
- **Model sample-build:** `live_forecast_pipeline.py` wrote **12** forecasts with `p_model` set (technical + contract_tape)
  - Mix: KXBTC×5 (incl. 2× trade_yes) + KXINX×7 (incl. 1× trade_yes); agents_opining=2 on all
  - Notable trade_yes: KXBTC-26JUL2013-B65250, KXBTC-26JUL2013-B65050, KXINX-26JUL20H1600-B7462
  - Unresolved book now has more model-bearing short-horizon rows (Jul20 13:00Z BTC + 16:00Z INX)
- **Shipped:** `scripts/live_forecast_pipeline.py` — scrub Hermes `PYTHONPATH`/`sys.path` hermes-agent entries before third-party imports
  - Cron host injects broken hermes-agent venv (missing `pydantic_core` native ext) which shadowed fincept-sidecar's pydantic
  - Fix verified: import succeeds with polluted PYTHONPATH when run under `fincept-sidecar/.venv`
- **Ops note:** run live pipeline with fincept venv:
  `fincept-sidecar/.venv/Scripts/python.exe scripts/live_forecast_pipeline.py`
- **Next cron:** resolve Jul20 13:00Z BTC hourlies + city highs + INX 16:00Z as they print; re-run live pipeline; open paper lots on high-conviction trade_yes (app UI) so PnL leg can trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — full gate correctly LOCKED on eligible sample + paper PnL

## Maintenance notes (2026-07-20, morning cron) — health green; **honest eligible gate reporting**

- Health: `cargo check`, `tsc` clean, **306** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Branch: `fix/edge-measurement-integrity` (ahead of master with honest-calibration work)
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **+28** Jul-19 city-high outcomes (CHI/DEN/LAX/MIA/NY/PHIL)
  - Progress raw: **255** resolved / **103** unresolved / **358** total (was 213/125/338)
  - [raw] Brier p_final **0.1578** vs p_market **0.1583** vs p_model **0.2860**
- **Honest gate (this pass):** branch `evaluate_gate` counts **eligible** rows only (p_model + pre-event + one/event)
  - **eligible = 19/200 (9.5%) — LOCKED** (prior "OPEN candidate" on raw n≥200 was misleading)
  - Eligible Brier p_final **0.3186** ≤ p_market **0.3257** (beats market on thin model sample)
  - `paper_lots` = **0** — PnL leg still unmet. Do **not** flip live execution.
- **Shipped:**
  1. `resolve_settled_forecasts.py` v1.2 — summary reports **eligible** gate progress (matches Rust), not raw resolved
  2. `live_forecast_pipeline.py` — fetches agent-analyzable series (KXBTC/KXETH/KXINX/…) instead of generic open book (was logging tennis with p_model=NULL)
- **Model sample-build (this pass):** `live_forecast_pipeline.py` wrote **12** forecasts with `p_model` set (technical + contract_tape)
  - Mix: KXBTC×5 (incl. 2× trade_yes) + KXINX×7; agents_opining=2 on all
  - Unresolved book now has model-bearing short-horizon rows that will advance eligible n when they settle
- **Next cron:** resolve Jul20 09:00Z BTC hourlies + city highs + INX 16:00Z as they print; re-run live pipeline for more p_model rows; open paper lots on high-conviction trade_yes (app UI or paper IPC) so PnL leg can trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; eligible n→200 + paper PnL>0
- **No Phase 1+ / Phase 5 code advancement** — full gate correctly LOCKED on eligible sample + paper PnL

## Maintenance notes (2026-07-19, evening cron) — health green; **n≥200 sample-size gate crossed**

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops (this pass):** `resolve_settled_forecasts.py` wrote **+72** outcomes in waves
  - Wave 1: **41** MLB finals — LAD@NYY game/totals + CWS@TOR game/team-totals/totals + TB@BOS KS (2 rows)
  - Wave 2: **2** KXWORLDCUPHALFTIME-26-POS
  - Wave 3: **9** NYM@PHI team-totals/totals (logged earlier this pass) + WORLDCUP SWA×2
  - Wave 4 (21:03Z): **18** Jul-19 17:00Z BTC/ETH hourlies (BTC 1Y/8N; ETH 1Y/8N) + WORLDCUP SAB×2
  - Progress: **213 / 200** resolved (was 141 late-afternoon; **+72** → **106.5%**)
  - Mean Brier: p_final **0.1553** vs p_market **0.1559** vs p_model **0.2860** (p_final beats market by +0.0006)
  - Script status: **OPEN candidate** (n≥200 AND Brier_final ≤ Brier_market)
- **Full Phase 3 gate (code `evaluate_gate`) still LOCKED:** requires paper PnL after fees > 0 — `paper_lots` = **0 rows**. Do **not** flip live execution.
- **Honesty / composition caveat:** all 213 resolved are `verdict=pass`; only **19/213** have `p_model` set. Book is dominated by market-only sample-build rows (`p_final≈p_market` by construction). Thin edge is real but not yet a model-edge proof.
- **Sample-build:** logged **40** short-horizon market-only forecasts
  - Mix: KXBTC 12 / KXMLBTEAMTOTAL 12 / KXETH 9 / KXMLBTOTAL 5 / KXHIGHPHIL 1 / KXHIGHDEN 1
  - Includes Jul-20 17:00Z BTC/ETH hourlies + BAL@HOU / MIA@MIL / NYM@PHI leftovers + city highs
  - Ledger: **338** total / **213** resolved / **125** unresolved
  - Unresolved close mix: Jul20=57, Jul21=32, Jul22=14 + long-dated leftovers
- **Resolved mix (all-time):** KXMLB* 85 / OTHER 39 / KXETH 33 / KXBTC 32 / KXHIGH* 18 / KXWORLDCUP* 6
- **Next cron:** keep resolve flywheel on Jul20 weather/crypto/MLB; prefer agent/`p_model` forecasts over more pure market-only rows; open a few paper lots on high-conviction passes so the PnL leg of `evaluate_gate` can eventually trip; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; full gate = paper PnL > 0 + sustained model-vs-market edge (not just n)
- **No Phase 1+ / Phase 5 code advancement** — sample-size sub-gate open; live money still correctly blocked

## Maintenance notes (2026-07-19, late-afternoon cron) — health green; calibration 141/200

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** waited through 18:00Z close; `resolve_settled_forecasts.py` wrote **15** Jul-19 14:00Z BTC/ETH hourly outcomes
  - BTC×7 (B64350–B65050; 1 Yes / 6 No) + ETH×8 (B1767–B1887; 0 Yes / 8 No)
  - Progress: **141 / 200** resolved (was 126 evening; +15 this pass → **70.5%**)
  - Mean Brier: p_final **0.1848** vs p_market **0.1859** vs p_model **0.3834** (p_final still slightly beats market; gate LOCKED)
- **Sample-build:** only **+1** new short-horizon row (book saturated; 103 candidates already unresolved/recent)
  - Logged: KXETH-26JUL1914-B1887 (resolved later this pass)
  - Ledger: **298** total / **141** resolved / **157** unresolved
  - Unresolved close mix: Jul19=18, Jul20=36, Jul21=30, Jul22=45 + long-dated leftovers
- **Shipped:** `scripts/resolve_settled_forecasts.py` v1.1 — `--poll-minutes` / `--poll-interval` / `--quiet-open` so crons can wait near hourly crypto closes without ad-hoc sleep scripts
- **Next cron:** re-run resolve after BTC/ETH 17:00Z hourlies (close 21:00Z) + Jul19 weather print overnight + any finalized MLB; sample-build only if short book thins; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; n→200 calibration gate (70.5%)
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-19, evening cron) — health green; calibration 126/200

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **9** Jul-19 13:00Z BTC/ETH hourly outcomes
  - BTC×4 (B64350–B64650; 1 Yes / 3 No) + ETH×5 (B1852–B1882; 0 Yes / 5 No)
  - Progress: **126 / 200** resolved (was 117 afternoon; +9 this pass → **63.0%**)
  - Mean Brier: p_final **0.2007** vs p_market **0.2019** vs p_model **0.3834** (p_final still slightly beats market; gate LOCKED)
  - Unresolved after resolve: **132** before sample-build
- **Sample-build:** logged **39** short-horizon market-only forecasts
  - Mix: KXMLBTEAMTOTAL 11 / KXMLBTOTAL 10 / KXBTC 7 / KXETH 7 / KXMLBGAME 2 / KXHIGHMIA 1 / KXHIGHDEN 1
  - Includes ~0.03d BTC/ETH 14:00Z hourlies (same-day resolve) + CWS@TOR game/totals/team totals + Jul-20 MIA/DEN highs
  - Ledger: **297** total / **126** resolved / **171** unresolved
  - Unresolved close mix: Jul19=32, Jul20=36, Jul21=30, Jul22=45 + long-dated leftovers
- **Next cron:** re-run resolve after BTC/ETH 14:00–17:00Z hourlies + Jul19 weather print + any finalized MLB (LAD@NYY / CWS@TOR still active on public API at pass time); optional another `--limit 40` if short book thins; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; n→200 calibration gate (63.0%)
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-19, afternoon cron) — health green; calibration 117/200

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **12** Jul-19 09:00Z BTC/ETH hourly outcomes
  - BTC×6 (B63950–B64450) + ETH×6 (B1857–B1887); 2 Yes / 10 No
  - Progress: **117 / 200** resolved (was 105 midday; +12 this pass → **58.5%**)
  - Mean Brier: p_final **0.2109** vs p_market **0.2123** vs p_model **0.3834** (p_final still slightly beats market; gate LOCKED)
  - Unresolved after resolve: **101** before sample-build
- **Sample-build:** logged **40** short-horizon market-only forecasts
  - Mix: KXHIGHNY 6 / KXHIGHCHI 6 / KXETH 5 / KXHIGHLAX 5 / KXBTC 4 / KXHIGHPHIL 4 / KXHIGHDEN 4 / KXHIGHMIA 3 / KXMLBTEAMTOTAL 3
  - Includes ~0.03d BTC/ETH 13:00Z hourlies (same-day resolve) + Jul-20 city highs + CWS@TOR team totals
  - Ledger: **258** total / **117** resolved / **141** unresolved
  - Unresolved close mix: Jul19=27, Jul20=36, Jul21=28, Jul22=22 + long-dated leftovers
- **Next cron:** re-run resolve after BTC/ETH 13:00Z hourlies + remaining Jul19 weather/MLB print; optional another `--limit 40` if short book thins; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; n→200 calibration gate (58.5%)
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-19, midday cron) — health green; calibration 105/200

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- Auto-remediation: none needed (clean tree)
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **18** Jul-18 city-high outcomes (CHI/DEN/LAX/MIA/NY/PHIL)
  - Progress: **105 / 200** resolved (was ~87 before this pass; evening notes showed 62 — overnight/app + prior resolves filled gap)
  - Mean Brier: p_final **0.2211** vs p_market **0.2226** vs p_model **0.3834** (p_final still slightly beats market; gate LOCKED)
  - Unresolved after resolve: **88** before sample-build
- **Sample-build:** logged **25** short-horizon market-only forecasts (KXMLBTOTAL 11 / KXBTC 6 / KXETH 6 / KXMLBGAME 2)
  - Includes ~0.02d BTC/ETH hourly strikes (should resolve same day) + LAD@NYY totals/game
  - Ledger: **218** total / **105** resolved / **113** unresolved
  - Unresolved close mix: Jul19=30, Jul20=36, Jul22=19 + long-dated leftovers
- **Next cron:** re-run resolve after BTC/ETH hourly + Jul19 weather print; optional another `--limit 40` if short book thins; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; n→200 calibration gate (52.5%)
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-18, evening cron) — health green; Prometheus brand + calibration flywheel

- Health: `cargo check`, `tsc` clean, **268** lib tests (0 failed, 9 ignored)
- Auto-remediation: committed coherent WIP **Prometheus luxury brand system** (obsidian/antique gold design tokens, Cinzel+Inter fonts, constellation backdrop, nav icons, LiveDot, chart restyle, brand assets/favicon). Dropped unused CSSProperties/LiveDot imports so tsc stays clean.
- KB-1: still 🟡 — code path fixed; live credential/UI acceptance still required on user machine
- KB-2: ✅ complete
- **Resolve ops:** `resolve_settled_forecasts.py` wrote **19** outcomes (BTC/ETH hourly + PIT@CLE MLB game/totals)
  - Progress: **62 / 200** resolved (was 43); unresolved **110** after sample-build (was 89 before resolve, then +40 logs)
  - Mean Brier: p_final **0.2575** vs p_market **0.2600** (p_final still slightly beats market; gate LOCKED)
- **Sample-build:** logged **40** more short-horizon market-only forecasts (KXBTC 12 / KXMLBTOTAL 12 / KXETH 10 / KXMLBTEAMTOTAL 4 / KXMLBGAME 2)
  - Ledger: **172** total / **62** resolved / **110** unresolved
  - Unresolved horizon mix: Jul18=4, Jul19=36, Jul20=36, Jul21=18 + long-dated leftovers
- **Next cron:** re-run resolve as crypto/MLB print; optional another `--limit 40` if short book thins; leave app running for auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; n→200 calibration gate
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)


## Maintenance notes (2026-07-18, afternoon cron) — health green; short-horizon sample-build

- Health: `cargo check`, `tsc`, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- KB-1: still 🟡 — code path fixed; live cache has **200** markets (fetched 2026-07-16); user UI acceptance still required
- KB-2: ✅ complete; Master sprints 0–7 + S8–S12 complete
- **Resolve ops:** `resolve_settled_forecasts.py --dry-run` → 0 new settles (16 long-dated actives still open; gate **43/200** LOCKED)
- **Shipped:** `scripts/log_short_horizon_forecasts.py` — public-API sample-builder for Phase 3 n→200
  - Honest market-only rows (`p_model=NULL`, `p_final=p_market`, `verdict=pass`) matching Rust pipeline offline path
  - Series filters (MLB/weather/crypto/INX), quote quality + mid∈(0.04,0.96), per-series diversity cap
  - Dedupes unresolved + last-12h tickers; dry-run supported
- **Live DB action:** logged **73** short-horizon forecasts (close mostly Jul 18–20)
  - Ledger: **132** total / **43** resolved / **89** unresolved (was 59/43/16)
  - Horizon mix unresolved: Jul18=7, Jul19=18, Jul20=48 + 16 long-dated leftovers
  - Mean Brier unchanged until settles land (p_final 0.3046 vs market 0.3082 on n=43)
- **Next cron:** re-run `resolve_settled_forecasts.py` as weather/crypto/MLB print; optional another `--limit 40` sample-build if short book thins
- Bare `tokio::spawn` only in paper unit tests (production uses `tauri::async_runtime`)
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; leave app running so auto-grade closes short-dated settles; n→200 now unblocked by sample pipeline
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-18, morning cron) — health green; resolved 5 settled MLB forecasts

- Health: `cargo check`, `tsc`, **268** lib tests (0 failed, 9 ignored); working tree **clean** at start
- KB-1: still 🟡 — code path fixed; live cache has **200** markets (fetched 2026-07-16); user UI acceptance still required
- KB-2: ✅ complete; Master sprints 0–7 + S8–S12 complete
- **Calibration ops (live DB `~/.openclaw/kalshi-monster/predictions.db`):** public Kalshi API showed 4 finalized MLB markets (5 forecast rows) still unresolved; wrote outcomes + Brier
  - Rows: 52–53 `KXMLBTEAMTOTAL-…TOR8` → No; 57 `KXMLBTOTAL-…CWSTOR-4` → Yes; 58–59 `KXMLBTOTAL-…SDKC-14/15` → No
  - Progress: **43 / 200** resolved (was 38); unresolved **16** (all long-dated actives: Senate/House/NCAAF/Oscar/Pres/etc.)
  - Mean Brier: p_final **0.3046** vs p_market **0.3082** vs p_model **0.3834** (p_final still slightly beats market on n=43; gate still LOCKED)
  - predictions table: 19 rows, **0 pending**; paper_lots: 0 open
- **Shipped:** `scripts/resolve_settled_forecasts.py` — reusable public-API resolve companion for when the desktop app is not running (dry-run supported). Matches Rust Brier math (outcome 1=Yes/0=No).
- Bare `tokio::spawn` only in paper unit tests (production uses `tauri::async_runtime`)
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; leave app running so auto-grade closes short-dated settles; n→200 (need short-horizon forecasts — long-dated book won't move the gate)
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-17, afternoon cron) — health green; resolved 3 settled forecasts

- Health: `cargo check`, `tsc`, **268** lib tests (0 failed, 9 ignored); working tree **clean**
- KB-1: still 🟡 — code path fixed; live cache has **200** markets (fetched 2026-07-16); user UI acceptance still required
- KB-2: ✅ complete; Master sprints 0–7 + S8–S12 complete
- **Calibration ops (live DB `~/.openclaw/kalshi-monster/predictions.db`):** public Kalshi API showed 3 finalized gold markets still unresolved; wrote outcomes + Brier for rows 48–50
  - Progress: **38 / 200** resolved (was 35); unresolved 21 (long-dated actives)
  - Mean Brier: p_final **0.3316** vs p_market **0.3358** vs p_model **0.4133** (p_final slightly beats market on n=38; gate still LOCKED)
  - All ledger verdicts remain `pass` (sample-building only; no paper lots open)
- Bare `tokio::spawn` only in paper unit tests (production uses `tauri::async_runtime`)
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets UI acceptance; leave app running so auto-grade closes short-dated settles; n→200
- **No Phase 1+ code advancement** — remaining plan items are true blockers (credentials/UI verify / data / operator)

## Maintenance notes (2026-07-17, cron) — health green; release-build hygiene

- Health: `cargo check`, `tsc`, **268** lib tests (0 failed, 9 ignored); tree dirty only with release-merge artifacts
- Auto-remediation: gitignore `tauri.conf.release.merged.json` / `.nobuild.json` + stray `hermes-gateway.service`
- Added `scripts/merge_release_config.py` — deep-merges release overlay + clears `beforeBuildCommand` (Windows MSYS pitfall)
- KB-1: still 🟡 — live Kalshi credential verification on user machine (not cron-automatable)
- KB-2: ✅ complete; Master sprints 0–7 + S8–S12 complete; forecast→outcome poller wired in auto-grade
- **Blocked next (ops):** operator AGPL public push; KB-1 live Markets acceptance; calibration n→200 resolved forecasts
- **No Phase 1+ code advancement** — plan-defined items remaining are true blockers (credentials / data / operator)

## Maintenance notes (2026-07-16, cron) — health green; docs reconciled

- Health: `cargo check`, `tsc`, **259** lib tests, vitest **55/55**; git tree clean at start
- KB-1: still 🟡 — live Kalshi credential verification on user machine (not cron-automatable)
- KB-2: ✅ complete; Sprints 0–7 + S8–S12 per MASTER-PLAN / sprint-6-7 roadmap
- Reconciled stale “Next: Sprint 0” table in `docs/fincept-integration-progress.md`
- **Next:** operator AGPL public push; KB-1 live Markets acceptance; calibration n→200

## Maintenance notes (2026-07-16, evening) — inline monster-edge-core

- Removed sibling crate `monster-edge-core/`; prop edge math inlined in `analysis/edge_calculator.rs`
- Calibrator artifact embedded at `analysis/calibrator.json` (was in removed crate)
- `calibration.rs` resolves override → embedded JSON → none; analysis tests green
- Tauri icon set refreshed (Prometheus branding assets + Android/iOS packs)
- **Next:** operator AGPL public push; live tape KB-1 credentials (user machine)

## Maintenance notes (2026-07-16) — integration secret health probes

- `check_integration_secrets_health` IPC: live HTTP probes for Brave + FRED (env fallback for FRED)
- Settings **Probe Brave & FRED** + status pills; `save_config` preserves `fred_api_key` in memory
- Security posture lists brave/fred in redacted field inventory
- **Next:** operator AGPL public push; live tape KB-1 credentials

## Maintenance notes (2026-07-16) — post-program gaps S8–S12

- Paper cash precheck before lot; Edge Board **Paper YES/NO (agent fair)**
- FRED secret in Settings + analyze context; Brave wired for deep web
- CLV/close on prediction cards; Brier CSV export; bankroll=paper equity button
- AssetSignal IPC; ModelDisagreement when LLM vs p_final ≥10pts; macro calendar caveats
- Dual-writer note: paper forecast stores `llm`/`llm_fair` in agent_breakdown
- **Next:** operator AGPL public push; live tape KB-1 credentials

## Maintenance notes (2026-07-15) — MASTER sprints 0–7 complete

- **6.4–6.5** Fee preview (`kalshiFees`), profit factor display, MarketDetail confirm + notify
- **7.1** `docs/AGPL-SIDECAR-SPLIT.md` + `scripts/split-fincept-sidecar.ps1`
- **7.2–7.3** Real-data agents only; gated `POST /asset-signal`
- Paper sim test + roadmap: `reports/sprint-6-7-paper-test-and-roadmap.md`

## Maintenance notes (2026-07-15) — Sprint 4+5 macro + calibration flywheel

- **4.x** `fincept-sidecar/agents/macro.py`: FRED map (CPI/Fed/payrolls/GDP/UNRATE); null if unmapped/no key/no threshold; pytest `test_macro.py`
- **4.3** Economic silent-weight note when macro null; orchestrator runs macro on standard/deep
- **5.1–5.3** Calibration flywheel card; gate dashboard model-vs-market; λ sample bar (n/50)
- **5.4** Settings: prefer stronger model for live TAKE (free tier ok for paper/WATCH)
- Set `FRED_API_KEY` env for live macro opinions

## Maintenance notes (2026-07-15, afternoon pass) — Sprint 6 paper product polish

- **6.1** Settings bankroll card: "Cash / bankroll.json" ledger explainer.
- **6.2** Paper portfolio: inline Grade vs Settle copy; auto-refresh after Analyst paper record via `KALSHI_PAPER_UPDATED` event; analytics error banner (no more silent blank).
- **6.3** Chat bankroll policy loaded into `extractPaperDecision` — client stake uses real Kelly caps from bankroll.json + max_bet_pct. Large-stake confirm dialog for TAKE ≥$250 or ≥75% of cap.

## Maintenance notes (2026-07-15) — Sprint 3 depth tiers + sidecar ops

- **3.1** `AnalysisDepth` on analyze input; board scan = quick (tape only); single Analyze = standard; Deep top 3 = deep + web
- **3.2** Bridge counters: last latency, agent_calls, opining_rate, signals totals; Settings Fincept card surfaces them
- **3.3** `tauri.conf.release.json` externalBin guard test; binaries README unchanged
- Sidecar orchestrator respects `context.depth`
- **Next:** Sprint 4 macro agent

## Maintenance notes (2026-07-15) — Sprint 1+2 agents + Edge Board

- **1.1** Technical series map expanded (BTC/ETH/index/majors); barrier strike from ticker; `horizon_days` in context
- **1.2** `fincept-sidecar/agents/news.py` + orchestrator; Rust attaches `web_snippets` on deep/single analyze
- **1.3** `silent_agent_weight_report` → verdict_reasons when routing mass on null agents
- **1.4** pytest `test_news_and_technical_null.py` (+ technical math)
- **2.x** Calibration **Edge Board**: rank by \|edge_net\|, agent drawer, Deep top 3; Analyst sidecar chip + deep button
- Pure rank helper + cargo tests; vitest Calibration 7 + Chat 2
- **Next:** Sprint 3 depth tiers / ops UX; Sprint 4 macro

## Maintenance notes (2026-07-15) — Sprint 0 paper journal trust

- **0.1** `PaperRecordResult` IPC (`prediction_id`, `lot_opened`, `lot_id`, `final_decision`, `stake`, `demotion_notes`, `paper_lots_blocked`); ChatView + MarketDetailPanel show lot vs journal truth
- **0.2** Forecast `close_time` from market tape close/expiry (not wall-clock)
- **0.3** Equity snapshots: open MV = mark or **cost-basis fallback**; analytics same; `profit_factor` capped at 999 (no Infinity JSON)
- **0.4** Breakers block **new paper lots** on daily-loss pause / hard disable; calibration `paper_only` demotion still opens lots
- Tests: `cargo test --lib paper::` **11/11** (incl. `equity_snapshot_uses_cost_basis_when_no_marks`); MarketDetailPanel **4/4**; `tsc` clean
- **Next:** Sprint 1 (technical coverage + news agent) per `docs/MASTER-PLAN.md`

## Maintenance notes (2026-07-15, cron pass) — auto-remediation + serde fix

- Committed coherent WIP: paper auto-settle/prediction sync, Phase A agent priors (`agent_priors`, `opinion_input`), IPC serde defaults, decision schema aliases + quality rails.
- **Fix:** removed erroneous `serde(rename_all = SCREAMING_SNAKE_CASE)` on `DecisionAction`/`ContractSide` (was serializing `TAKE` as `T_A_K_E`, breaking 8 lib tests).
- Gitignore: `scripts/_*.py` ephemeral helpers.
- Verified: `cargo check`, `tsc`, **249** lib tests, vitest **46/46**, paper **10/10**.
- **Next:** Sprint 0 done → Sprint 1 agents; KB-1 live credential verify (user).

## Maintenance notes (2026-07-15) — paper auto-settle + prediction sync

(Merges Fincept deep integration + paper audit + analyst quality. Next: Sprint 0 paper trust, then Sprint 1 agents.)

## Maintenance notes (2026-07-15) — master plan merged

- Created `docs/MASTER-PLAN.md`: chronological sprints 0–7 from Fincept plan (B–D remaining) + paper audit leftovers + calibration flywheel.
- Pointers updated in `docs/fincept-integration-progress.md`.

## Maintenance notes (2026-07-15) — paper auto-settle + prediction sync

- `paper_lots.prediction_id` column + link on open
- `settle_pending`: side-aware close + **sync prediction Win/Loss/PnL** + resolve forecasts
- Auto-grade poller always settles open paper lots; Grade / Resolve IPC also settle paper
- Tests: paper:: **10/10** (`settle_syncs_linked_prediction_outcome`)
- Audit updated: `docs/paper-system-audit-2026-07-15.md`

## Maintenance notes (2026-07-15) — paper system audit + critical fixes

- Audit: `docs/paper-system-audit-2026-07-15.md`
- **Critical fix:** NO-side settlement was inverted (used YES exit cents for all lots)
- **High:** lots only open on `decision=TAKE` (not WATCH); prediction `entry_price` stores dollars not market %
- **IPC:** serde defaults for evidence/risk_flags/data_quality; `ChatExtract` quality; risk `#[serde(other)]`; case aliases
- Tests: paper:: **9/9** including `no_side_settlement_pays_on_no_result`
- **Next paper:** equity MTM snapshots; structured IPC response

## Maintenance notes (2026-07-15) — Fincept Phase A (agent priors in Analyst)

- Plan in-repo: `docs/fincept-sidecar-deep-integration-plan.md` (Phases A–D for other agents).
- Shipped Phase A: `edge_engine/opinion_input` (mids + underlying/strike), `chat/agent_priors` injects SIDECAR MODEL PRIORS into chat; Analyze uses shared builder; chat forecasts prefer edge pipeline + LLM annotation.
- **Next:** Phase B (news/macro/technical coverage), Edge Board (Phase C).

## Maintenance notes (2026-07-15) — comprehensive model performance (no math)

- Analyzed latest Analyst sessions + `predictions.db`: free DeepSeek **D+** process grade; edge ledger 35/35 PASS; graded track record thin (1 Loss / 14 Pending).
- **Wave 1:** bid/ask/mid labels; retrieval; JSON-first prompts; deliverable strip; resend mode; last-valid JSON; quality rails; paper rails.
- **Wave 2 (comprehensive):** `chat/track_record` prompt card; free-tier model note; stream thoughts separate from content; chat→forecast ledger; extract dedupe/repair; grade Yes/No normalize + skip placeholders; UI collapsible thinking; paperFromChat quality rails + preferDeliverable.
- Report: `reports/model-session-performance-2026-07-14.md`.
- Verified: `cargo test --lib chat::` **63**; `kalshi::grading` **7**; `predictions::tracker` **6**.
- **Next:** run resolve poller on user machine to grade open markets; stronger model for live TAKE; optional agent fill rates.

## Maintenance notes (2026-07-14, cron pass) — calibration flywheel vitest + binaries .gitkeep

- Shipped: `CalibrationView.test.tsx` +1 test for **Resolve settled forecasts** (IPC + post-resolve report refresh); track empty `src-tauri/binaries/` via `.gitkeep` (exe remains gitignored).
- Auto-remediation: committed untracked `.gitkeep` per `binaries/.gitignore` `!.gitkeep`.
- Verified: `python scripts/build_fincept_sidecar.py --check-env` green; CalibrationView vitest **4/4**; `cargo check`, `tsc`, **223** lib tests.
- **Next:** KB-1 logged-in portfolio verify (user); `tauri build --config tauri.conf.release.json`; accumulate resolved forecasts for calibration gate.

## Maintenance notes (2026-07-14, cron pass) — Phase 1 sidecar PyInstaller + Hermes-safe build

- Shipped: `scripts/build_fincept_sidecar.py` always uses `fincept-sidecar/.venv` interpreter; strips Hermes `PYTHONPATH` pollution; `--check-env` + `--self-test`; `fincept-sidecar` optional `[bundle]` deps (`pyinstaller`, `pywin32-ctypes`); gitignore PyInstaller artifacts; binaries README cron notes.
- Verified: `--check-env` green; `--self-test` green; full PyInstaller build staged `kalshi-monster/src-tauri/binaries/fincept-sidecar-x86_64-pc-windows-msvc.exe` (~42MB, gitignored).
- Health: `cargo check`, `tsc`, **223** lib tests; tree was clean at start.
- **Next:** KB-1 logged-in portfolio verify (user); `tauri build --config tauri.conf.release.json`; calibration flywheel.

## Maintenance notes (2026-07-14, cron pass) — stream token estimate + Phase 1 packaging test

- Shipped: fix `unused_assignments` in `chat/openrouter.rs` (stream token estimate computed once after auto-continue loop); +1 lib test `staged_sidecar_artifact_name_matches_build_script` pins `build_fincept_sidecar.py` ↔ Tauri `binaries/` layout.
- Verified: `python scripts/build_fincept_sidecar.py --dry-run` ok (`x86_64-pc-windows-msvc`).
- Health: `cargo check` (0 warnings), `tsc`, **223** lib tests (+1); tree was clean at start.
- **Next:** KB-1 live credential verification (user); full PyInstaller sidecar + `tauri build --config tauri.conf.release.json`; calibration flywheel.

## Maintenance notes (2026-07-14, overnight) — fleet backlog updated; KB-2 marked complete

- Documentation: updated `docs/fleet-backlog-2026-07-08.md` — KB-2 (Analyst tab) now marked ✅ Complete. All 5 slices shipped: KB-2a (context chip + structured status), KB-2b (session rename), KB-2c (paper hook from chat), KB-2d/e (streaming + error retry + live quick prompts).
- Health: `cargo check`, `tsc`, **222** lib tests all green; working tree clean (auto-remediated last pass).
- KB-1 status: 🟡 root cause fixed (dual-runtime blocking RwLock); remaining blocker is live credential verification on user machine — not automatable in a cron pass.
- **Next:** KB-1 live credential verification (user); calibration flywheel (accumulate resolved forecasts); Phase 1 sidecar packaging.

## Maintenance notes (2026-07-13, cron pass) — KB-1 bootstrap tests + sidecar dry-run

- Shipped: +2 lib tests for dashboard empty-tape login vs public-catalog hints; +1 test pinning `market_count` to full tape size (not top-N slice only); `build_fincept_sidecar.py --dry-run` for packaging layout checks without PyInstaller.
- Health: `cargo check`, `tsc`, **222** lib tests (+2), bootstrap test module 7/7 green; sidecar dry-run ok on `x86_64-pc-windows-msvc`.
- **Next:** KB-1 logged-in portfolio verify on user machine; run full `build_fincept_sidecar.py` + `tauri build --config tauri.conf.release.json`; calibration flywheel.

## Maintenance notes (2026-07-13, cron) — KB-1 sync tests + release externalBin config

- Shipped: unit tests for `sync_kalshi_client_from_app_config` (login gate + cache invalidation on credential change); `tauri.conf.release.json` with `externalBin: ["fincept-sidecar"]` (pairs with `scripts/build_fincept_sidecar.py` + `src-tauri/binaries/README.md`); Fincept bridge test for bundled exe name.
- Health: `cargo check`, `tsc`, **220** lib tests (+3), vitest **40** green.
- **Next:** KB-1 live credential verification on user machine; run `python scripts/build_fincept_sidecar.py` then `tauri build --config tauri.conf.release.json` for packaged sidecar; calibration flywheel.

## Maintenance notes (2026-07-13, cron 4pm) — KB-1 bootstrap config sync

- Shipped: `sync_kalshi_client_from_app_config` on `kalshi_get_dashboard_bootstrap` (and shared with portfolio/refresh); empty-tape hints distinguish login vs public-catalog paths; +1 lib test.
- Auto-remediation: gitignored `scripts/add_*.py` (stray `add_rename_ui.py`).
- Health: `cargo check`, `tsc`, **217** lib tests (+1), KalshiView vitest **14** green.
- **Next:** KB-1 live credential verification on user machine; calibration flywheel; `externalBin` sidecar packaging.

## Maintenance notes (2026-07-13, overnight) — session rename (KB-2b)

- Shipped: `rename_session()` backend + `rename_chat_session` Tauri command + inline rename UI in ChatView (double-click pencil → inline input). Completes the KB-2b layout/sessions slice.
- Health: `cargo check`, `tsc`, **216** lib tests green. KB-2 essentially done (2a/2b/2c/2d).
- **Next:** KB-1 live credential verification (blocking); calibration flywheel; Phase 1 sidecar packaging.

## Maintenance notes (2026-07-12, cron) — fee-aware grading + paper PnL (§6.5)

- Shipped: `contract_pnl` / `evaluate_bet` use persisted `fee_multiplier`; paper `place_trade` charges `order_fee` on open; `order_fee` accepts fractional contracts; predictions storage aligned.
- Auto-remediation: committed coherent interrupted WIP (4 Rust modules).
- Health: `cargo check`, `tsc`, **211** lib tests, **40** vitest green.
- **Next:** calibration flywheel; `externalBin` sidecar packaging.

## Maintenance notes (2026-07-12, cron pm b) — Fincept bridge Settings panel

- Shipped: Settings **Fincept sidecar (Phase 1)** card — status, start/stop dev, refresh via existing IPC.
- Health: `tsc`, **40** vitest green.
- **Next:** calibration flywheel; `externalBin` sidecar packaging.

## Maintenance notes (2026-07-12, cron pm) — secrets keyring + forecast→outcome bridge

- Shipped: `secrets.rs` OS credential store (keyring); migrate plaintext API keys from `config.json`; Settings load/save via `getSecrets` / `saveSecret` IPC; auto-grade calls `forecast::resolve_forecasts_for_market` when Kalshi markets settle; paper transaction helpers.
- Auto-remediation: committed coherent interrupted WIP from prior pass (13 files + `secrets.rs`).
- Health: `cargo check`, `tsc`, **207** lib tests, **39** vitest green.
- **Next:** calibration flywheel (accumulate resolved forecasts).

## Maintenance notes (2026-07-12, cron) — full edge_config Settings + IPC

- Shipped: `save_edge_config` / `kalshi_set_edge_config` for all five EdgeConfig fields; Settings **Edge engine config** card with Save all; NaN sentinels for unchanged fields; refit λ uses NaN for non-λ fields; vitest mock `setEdgeConfig`.
- Auto-remediation: committed interrupted WIP; gitignored one-off `scripts/fix_*.py` / `expand_*.py` agent helpers.
- Health: `cargo check`, `tsc`, 202 lib tests, **39** vitest green.
- **Next:** KB-1 live credential verification; forecast→outcome bridge on auto-grade; calibration flywheel.

## Maintenance notes (2026-07-11, cron) — Settings manual shrinkage λ override

- Shipped: `kalshi_set_shrinkage_lambda` IPC; Settings **Edge engine** card loads/saves persisted λ; vitest for card visibility.
- Health: `cargo check`, `tsc`, lib tests, **39** vitest green.
- **Next:** calibration flywheel; expand other edge_config fields in Settings when persisted.

## Maintenance notes (2026-07-11, cron) — edge λ persistence + paper breaker tests

- Shipped: `edge_config` table, `kalshi_get_edge_config`, refit persists λ; analyze/paper use loaded config; `paper_breaker` module + tests; Calibration UI active λ.
- Health: `cargo check`, `tsc`, 202 lib tests, 38 vitest green.
- **Next:** calibration flywheel; optional manual λ override in Settings.

## Maintenance notes (2026-07-11, cron) — Phase 3 λ UI + paper breaker stake scale

- Shipped: `kalshi_refit_lambda`, `CircuitBreakerActive`, paper-path `stake_multiplier`, Calibration λ panel + vitest.
- Health: `cargo check`, `tsc`, 198 lib tests, 38 vitest green.
- **Next:** calibration flywheel; optional persist fitted λ to edge config.

## Maintenance notes (2026-07-10, cron) — Analyst settlement gates + web evidence

- Committed interrupted WIP: `market_gate`, `web_context`, decision enforcement, `brave_api_key`, `paperFromChat` tests.
- Health: `cargo check`, `tsc`, 195 lib tests, 37 vitest green.
- **Next (per progress doc):** reliability diagram on Calibration tab; `live_orders_allowed` on order path.

## Maintenance notes (2026-07-10, cron) — Phase 3 breaker persistence

- SQLite `breaker_state` + `evaluate_and_persist_breakers`; Tauri IPC + Calibration tab §6.4 panel.
- See `docs/fincept-integration-progress.md` for current next steps.

**Next-work source of truth:** `docs/fincept-integration-progress.md` → **Current next steps**.
This file is a reverse-chronology maintenance log; do not treat scattered “Next:” bullets below as current if they conflict with the progress doc.

## Maintenance notes (2026-07-10) — Analyst completeness + retrieval

- **Early stop:** raised completion budget to 16k + auto-continue until decision JSON/summary (chat log cut mid-thought).
- **Retrieval:** query-filtered markets (8), gated Fincept spots, ML only for sports; gold labeled as futures with as_of.
- **Stream UX:** full-width plain stream body; first tokens from reasoning channel mirrored to content.
- **Next:** calibration flywheel; Phase 3 productization.

## Maintenance notes (2026-07-10) — Analyst stream polish + OpenCode

- **Empty Analyst replies:** OpenCode Zen free models stream into `reasoning` only; promote to `content` on save.
- **LLM providers:** OpenRouter / OpenCode Zen / OpenCode Go in Settings (`30b93a3`).

## Maintenance notes (2026-07-09) — agents, forecast ledger wiring, KB-1 confirmed

- **KB-1 root cause confirmed + fixed:** `shared_cache.blocking_write()` / `blocking_read()` on Tokio `RwLock` from inside `tauri::async_runtime` (after successful HTTP fetch in `ensure_quick_cache`/`store_cache`) panics with "Cannot block the current thread from within a runtime" — markets never landed in cache. Replaced with `.write().await` / `try_read`. Tests lock the panic and the async path. Public Kalshi `/markets` returns data **without** credentials; portfolio still needs login.
- **Agents (real data):** `technical` (yfinance) + `contract_tape` (Kalshi mids in context); orchestrator + `POST /api/v1/agents/market-opinion`.
- **Ledger:** `edge_engine::pipeline` + paper path fill `p_market`/`p_model`/`p_final`/`verdict`; predictions table migrated to mirror; IPC for analyze/resolve/calibration report.
- **Live evidence:** 8 open forecast rows written via `scripts/live_forecast_pipeline.py` from live Kalshi API — **0 resolved** (honest; gate not claimable).

## Maintenance notes (2026-07-09, cron) — Phase 3 calibration core + §6.4 breakers

Scheduled-task directive was "highest effort/reasoning areas of the plan"; KB-1's sole
remaining item needs live Kalshi credentials (not verifiable in an automated pass) and
KB-2b–e are UX slices, so this pass shipped the plan's Phase 3 mathematical core instead.

- **`edge_engine/calibration.rs` (new):** Brier summaries (incl. apples-to-apples
  market-restricted-to-model-rows mean), 10-bucket reliability diagram (p=1.0 lands in
  bucket 9), **λ re-fit** by deterministic 0.001-grid argmin of mean Brier of the
  *re-shrunk* `shrink(p_model, p_market, λ)` (§4.1; ties break toward smaller λ; requires
  ≥50 model rows — `LAMBDA_REFIT_MIN_SAMPLES`), **calibration gate** (§7 Phase 3: ≥200
  resolved AND Brier(p_final) ≤ Brier(p_market) AND paper P&L > 0, per-condition
  reporting), and **rolling-50 degradation check** (§6.4 last row; `None` below a full
  window — breakers must not trip or clear on partial data).
- **`edge_engine/breakers.rs` (new):** §6.4 as a pure state machine
  `(prev, inputs, cfg) → decision`. Daily-loss pause stateless (strict >5%); 15% drawdown
  scaler with **hysteresis** (arms >15%, releases <10%, band retains); 25% breaker
  **latches** until `manual_reenable` (re-latches if still in drawdown); calibration
  degradation latches until a full *healthy* window (absence of evidence ≠ recovery).
  `live_orders_allowed` encodes §6.5 invariant #2; should-fail test included.
- **`kalshi/forecast.rs`:** `resolved_forecasts_for_calibration` accessor (sorted
  ascending by `resolved_at` — the ordering contract `rolling_degradation` requires) +
  out-of-order resolution test.
- **Tests:** 18 new (13 calibration, 5 breakers incl. invariant sweep) + accessor test;
  **51 pass** in module-shim harness (sandbox lacks webkit deps for full `cargo check` —
  edge_engine + forecast modules compiled standalone against serde/serde_json/sqlx 0.8,
  same versions as Cargo.toml). **Run full `cargo check` + `cargo test` on the host to
  confirm workspace integration** (expected clean: new code touches only `pub mod`
  registrations and one additive accessor).
- **Next (Phase 3):** wire IPC — `get_calibration_report` (BrierSummary + reliability +
  gate) and breaker state persistence + evaluation in the order path; then the
  auto-analysis universe loop and Calibration tab (§9).

## Maintenance notes (2026-07-09, cron KB-1) — bootstrap tape count + warm failures

- **`cached_tape_market_count`:** bootstrap `market_count` uses full tape size (not only the visible slice); `data_quality_notes` use tape count for "No markets loaded".
- **Background full warm:** `lib.rs` sets `last_fetch_error` when `fetch_all_markets` fails at startup (parity with quick-cache warm).
- **`ensure_quick_cache`:** when full catalog warm is in progress and tape is empty, stores a user-facing fetch hint for the UI.
- **Tests:** `cached_tape_market_count_reflects_cache_len` lib test; vitest `empty bootstrap surfaces last catalog fetch error`.
- **KB-1 remaining:** Live run with valid Kalshi credentials — confirm ≥ `INITIAL_MARKET_LIMIT` rows on Markets tab.

## Maintenance notes (2026-07-09, cron KB-1) — catalog fetch diagnostics

- **`KalshiClient`:** `last_fetch_error` field set on quick/full fetch failures and zero-market responses; cleared on successful non-empty cache.
- **`build_kalshi_dashboard_data_quality_notes`:** appends **Last catalog fetch error:** when client has a stored error; bootstrap passes `client.last_fetch_error()`.
- **`send_message_stream`:** forward task uses `tauri::async_runtime::spawn` (was bare `tokio::spawn`) — aligns with KB-1 spawn audit.
- **Startup warm:** `lib.rs` records `set_last_fetch_error` when `ensure_quick_cache` fails at boot.
- **`KalshiView`:** empty-tape error prefers the concrete fetch error string from `data_quality_notes`.
- **Tests:** `data_quality_notes_include_stale_and_fetch_hints` asserts fetch-error note; `cargo check` + `tsc` clean.

## Maintenance notes (2026-07-09, cron KB-2a) — structured degraded context IPC

- **`KalshiChatContextStatus` + `assess_kalshi_chat_context`** (`chat/kalshi_context.rs`): `degraded`, `tape_market_count`, `reasons` when tape empty or fetch failed.
- **Tauri event `chat-kalshi-context`:** emitted from `send_message` and `send_message_stream` before `build_kalshi_context` (`emit_chat_kalshi_context`).
- **Command `kalshi_get_chat_context_status`:** Analyst can poll tape readiness without sending a message.
- **UI:** `useChat` listens for `chat-kalshi-context` (session-scoped) + polls on init/send; `ChatView` shows structured amber banner with backend `reasons`.
- **Tests:** `assess_chat_context_degraded_when_tape_empty` passes (4/4 `kalshi_context` tests).

## Maintenance notes (2026-07-09, overnight cron KB-2a) — Analyst market context UX

- **ChatView:** `extractTickerFromPrompt` helper parses ticker from "Analyze Kalshi market <TICKER>: <title>" prompt string.
- **Context chip:** When arriving from Markets → "Analyze with AI", shows a blue chip with 🔍 ticker + title + dismiss button, plus a hint that "AI sees live Kalshi market data."
- Quick prompts remain generic placeholders; contextual follow-ups deferred to KB-2c.
- **KB-2a (legacy heuristic banner):** superseded by structured backend status above.

## Maintenance notes (2026-07-08, cron KB-1) — tape populate reliability
- `schedule_persist` uses `tauri::async_runtime::spawn` instead of bare `tokio::spawn` so SQLite cache writes run on the Tauri reactor.
- `ensure_quick_cache` refetches when persisted cache has **zero** markets (not only when stale).
- Bootstrap `data_quality_notes` includes **No markets loaded** when `market_count == 0`.
- `KalshiView`: empty tape `role="alert"` + **Retry refresh**; vitest `empty bootstrap shows credential hint and retry refresh`.
- **137** lib tests (data_quality_notes); **7** KalshiView vitest; `cargo check` + `tsc` clean.
- **KB-1 remaining:** Confirm ≥ `INITIAL_MARKET_LIMIT` rows with valid Kalshi credentials on a live run.

---

## Fleet backlog (2026-07-08) — cron priority

**Source:** `docs/fleet-backlog-2026-07-08.md` + integration plan §13 items 6–7.

| ID | Issue | Status |
|----|--------|--------|
| **KB-1** | Markets not populating in UI; suspected tokio/async spawn | 🟢 Root cause fixed: blocking RwLock write in async catalog path (verify UI once after rebuild) |
| **KB-2** | Analyst tab (`ChatView`) — major UX/context work | 🟡 Partial (KB-2a done: chip + `chat-kalshi-context` + poll command; KB-2b-e open) |

**Cron rule:** One KB-* slice per pass; **KB-1 before KB-2** until markets populate with valid credentials.

---

## Maintenance notes (2026-07-08, maintenance pass) — Phase 1: sidecar tracker + chat/UI wiring
- Committed prior-session WIP: `tracker.py` + market routes (`/tracker`, `/snapshot`); `FinceptBridge::get_json`; `fincept_context` appended after Kalshi context in chat send/stream; `get_fincept_market_tracker` IPC; **World markets** tab + `WorldMarketsView.tsx`.
- Sidecar: `uv run pytest tests/test_tracker.py` — **3 passed**; added `fincept-sidecar/uv.lock`.
- `.gitignore`: `*.egg-info/`.
- **130** lib tests pass; `tsc` clean; `cargo check` clean.
- **Next (Phase 1):** Settings panel hooks for bridge start/status; expand tracker toward plan Appendix A; ledger PASS / shrinkage columns (Phase 0 delta per progress doc).

## Maintenance notes (2026-07-08, overnight pass) — Phase 1: FinceptBridge auto-spawn at startup + background health supervisor
- Wired sidecar auto-spawn at app startup: `lib.rs` clones `fincept_bridge` before `setup(move |app|)`, then spawns `start_dev_sidecar()` in a tokio task at setup time.
- Background health supervisor: polls `/api/v1/health` every 60 s; on failure, `record_health_failure()` triggers restart (up to 3/10 min) before marking degraded.
- `externalBin` registration in `tauri.conf.json` deferred — Python sidecar uses `python main.py` dev path; PyInstaller bundling is a later packaging task.
- **129** lib tests pass; `tsc` clean; `cargo check` clean.
- **Next (Phase 1):** wire the sidecar into `MarketContextBuilder` (feed live market data to chat context); "World Markets" tab in React UI.

## Maintenance notes (2026-07-07, maintenance pass) — Phase 1: FinceptBridge supervisor
- Added `src-tauri/src/fincept_bridge/mod.rs`: READY-line parser, per-launch token, 30s handshake timeout, bearer health check to `/api/v1/health`, restart budget (3 / 10 min) + degraded flag.
- Dev spawn via `python` + `../../fincept-sidecar/main.py` (Tauri `externalBin` sidecar packaging deferred).
- IPC: `get_fincept_bridge_status`, `fincept_bridge_start_dev`, `fincept_bridge_stop`.
- `tokio` features: `process`, `io-util` for async stdout handshake.
- **4** new unit tests (+1 ignored integration); **129** lib tests pass total.
- **Next (Phase 1):** wire app startup spawn + background health supervisor; register `fincept-sidecar` in `tauri.conf.json` `externalBin` for packaged builds.

## Maintenance notes (2026-07-07, maintenance pass) — Phase 1 edge_engine + fincept-sidecar scaffold; 125 lib tests
- Committed uncommitted WIP from prior pass: Rust `edge_engine` module (shrinkage, Kalshi fee model, aggregation, Kelly sizing; 22 unit tests) registered in `lib.rs`.
- Added `fincept-sidecar/` FastAPI scaffold (auth, schemas, market-data engine, handshake tests) per plan Phase 1; copied plan + progress docs into `kalshi-monster/docs/`.
- `.gitignore`: ignore `.pytest_cache/`.
- **125** lib tests pass; `tsc` clean; `cargo check` clean.
- **Next (Phase 1):** `FinceptBridge` Rust supervisor (spawn sidecar, READY handshake, health/restart) — not started this pass (scope >30 min with Tauri `externalBin` wiring).

## Maintenance notes (2026-07-07, maintenance pass) — Phase 0: Forecast resolution bridge
- Wired the forecast ledger to Kalshi settlement: `resolve_forecasts_for_market` resolves all open rows per ticker with Brier scores.
- `grade_pending_predictions` now takes `db_pool` and resolves forecasts when prediction grading sees a settled market.
- `resolve_pending_forecasts` + auto-grade task poll when only forecast rows are pending (no prediction rows).
- `kalshi_grade_pending_predictions` IPC passes `db_pool` for manual grade parity.
- +1 lib test (`resolve_all_for_ticker`). **103** lib tests pass.
- Phase 0 status: resolution poller **complete** for forecasts + predictions; exit criteria met pending live settled data accumulation.

## Maintenance notes (2026-07-07, overnight cron — Phase 0 forecast ledger shipped; 102 lib + 20 vitest)
- Shipped the forecast ledger (`kalshi/src/kalshi/forecast.rs`) per the Fincept integration plan's Phase 0 schema: `forecasts` table with market ticker, timestamps, p_market/p_model/p_final, verdict (trade_yes/trade_no/pass), reasons, stake, agent breakdown, and resolution columns with Brier scores.
- Every `kalshi_record_paper_decision` now writes a forecast row — any opinion (YES, NO, or PASS) gets a row. This is the data every later phase (calibration, edge engine) depends on.
- Added +3 lib tests (insert, resolve+compute Brier, pass-outcome). 102 lib tests pass.
- Phase 0 status: 5 of 5 items done (market board live; `build_kalshi_context` wired; `ml_predictor` kept; forecast ledger shipped; resolution poller bridges forecasts on settlement).

Working copy: `C:\\Users\\ethan\\kalshi-build\\kalshi-monster`

Quick status: **P0 done · P1 done · P2 done · P3 1 pending**

---

## Maintenance notes (2026-07-06, afternoon cron pass) — Health verification

- Re-verified: `cargo check`, `tsc`, **99** lib tests, vitest **20**; working tree clean on `master`.
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no unblocked code work (clean-tree blocked-P3 policy).

## Maintenance notes (2026-07-06, cron pass) — Health verification

- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no unblocked code work (clean-tree blocked-P3 policy).
- Health: `cargo check`, `tsc`, **99** lib tests; vitest **20**; working tree clean on `master`.
- Reviewed `PRIORITIES.md` / `ROADMAP.md`: only open backlog item remains runtime data accumulation.

## Maintenance notes (2026-07-05, afternoon pass) — Planning doc sync

- **`ROADMAP.md`:** Maintenance cadence branch corrected to `master`; vitest noted in health checklist.
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live `predictions.db` — no code change (per clean-tree blocked-P3 policy).
- Health: `cargo check`, `tsc`, **99** lib tests; vitest **20**; `agent-healthcheck.ps1` PASS.

## Maintenance notes (2026-07-05, maintenance pass) — Repo hygiene

- **`.gitignore`:** Ignore `**/predictions.db` so dev copies under `src-tauri/` are not committed (canonical DB remains `~/.openclaw/kalshi-monster/predictions.db`).
- **P3:** Multi-category ML still gated on ≥10 graded rows per politics/economics/weather in live DB — no code change this pass.
- Health: cargo check, tsc, **99** lib tests; vitest **20** green.

## Maintenance notes (2026-07-04, maintenance pass) — Trading posture test coverage

- **`KalshiView.tradingPosture.test.ts`:** Unit tests for `tradingPostureFromTape` priority (in-progress > stale > snapshot > partial > full > warming).
- **`KalshiView.test.tsx`:** Integration test asserts **Stale tape** trading posture card when only stale `data_quality_notes` (no in-progress override).
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **12** green (6 posture unit + 6 component).

## Maintenance notes (2026-07-03, maintenance pass) — Trading posture tape hints

- **`KalshiView`:** `tradingPostureFromTape` drives the accent **Trading posture** card from `data_quality_notes` (in-progress refresh, stale tape, snapshot paint, partial/full) — same priority as decision tips.
- **Vitest:** Snapshot/stale/in-progress test asserts **Catalog updating** headline + body.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-03, maintenance pass) — In-progress refresh decision tip

- **`KalshiView`:** Insight rail decision tips mirror `data_quality_notes` when live catalog refresh is in progress (parity with stale/snapshot hints).
- **Vitest:** Extended snapshot/stale tape test to assert in-progress refresh tip.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-03, maintenance pass) — Dashboard tape quality hints

- **`commands/mod.rs`:** `build_kalshi_dashboard_data_quality_notes` adds stale-cache (>60s) and in-progress refresh hints on bootstrap; +2 unit tests.
- **`KalshiView`:** Decision tips mirror persisted-snapshot and stale-tape `data_quality_notes` (no new bootstrap struct fields).
- **Vitest:** Asserts snapshot/stale hints in status strip + insight tips.
- Health: cargo check, tsc, **99** lib tests pass; KalshiView vitest **5** green.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard active sidecar CV

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `active_sidecar_models` via lightweight `per_category_models` parse from unified `_meta.json`; +1 unit test.
- **`KalshiView`:** Insight rail **Sidecar data** card lists active sidecars with samples and CV (Settings parity).
- **Vitest:** Asserts active sidecar line on bootstrap mock.
- Health: cargo check, tsc, **97** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-03, overnight pass) — Priority path review
- Reviewed the frontend hint path for dashboard snapshot accuracy.
- Decision: defer cross-exposure / stage-name fields until the older SQLite-WAL backend path is directly validated; reuse the existing `data_quality_notes` surface instead of expanding the struct prematurely.
- Health: cargo check, tsc, **97** lib tests pass; no new code committed beyond this planning note.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard unified ML CV

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `unified_cv_accuracy_mean/std` and `unified_trained_at` via lightweight `_meta.json` read in `phase3_dashboard_summary`; +1 unit test.
- **`KalshiView`:** Status strip ML artifacts label shows unified CV ± std; insight rail shows trained date and ROADMAP data-metric line when ready.
- **Vitest:** CV and trained-date assertions on bootstrap mock.
- Health: cargo check, tsc, **96** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-02, maintenance pass) — Dashboard sidecar category progress

- **`MLPhase3DashboardSummary`:** Adds `non_sports_category_stats` (Politics/Economics/Weather graded counts) in bootstrap SQL path.
- **`KalshiView`:** Insight rail **Sidecar data (Kalshi paper)** card mirrors Settings per-category progress without opening Settings.
- **Vitest:** Asserts sidecar insight card when bootstrap includes category stats.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard ML artifact hints

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `unified_model_on_disk` / `active_sidecar_count` via `ml_artifacts_on_disk_summary` in `phase3_dashboard_summary`.
- **`KalshiView`:** Status strip shows ML artifacts; insight rail tips for pending grades and next sidecar unlock.
- **Vitest:** Artifact label assertion on bootstrap mock.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard Train ML CTA

- **`KalshiView`:** When `ml_phase3.auto_retrain_eligible`, status strip shows **Train ML models** (`ml_train_model` IPC, refreshes bootstrap, flashes sample/CV summary).
- **`KalshiView.test.tsx`:** Vitest for dashboard ML train action (4 tests).
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-07-01, maintenance pass) — Dashboard grade pending CTA

- **`KalshiView`:** When `ml_phase3.kalshi_pending_predictions > 0`, status strip shows **Grade N pending** (calls `kalshi_grade_pending_predictions`, refreshes bootstrap, flashes W/L/PnL summary).
- **`index.css`:** Compact `.smallGradeBtn` styling in `.dashboardStatus`.
- **Vitest:** New test for dashboard grade action.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-06-30, maintenance pass) — Dashboard Phase 3 auto-retrain hint

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary` adds `auto_retrain_eligible` / `resolved_until_auto_retrain` (total resolved SQL in bootstrap); `build_phase3_dashboard_summary` extended; test assertions updated.
- **`KalshiView`:** Diagnostic strip shows pending Kalshi grades and auto-retrain readiness (parity with Settings ML card).
- Health: cargo check, tsc, **95** lib tests pass; KalshiView vitest green.

## Maintenance notes (2026-06-30, maintenance pass) — Kalshi dashboard Phase 3 hint

- **`ml_predictor.rs`:** `MLPhase3DashboardSummary`, `phase3_dashboard_summary` / `build_phase3_dashboard_summary` (SQL-only, no joblib read); +1 unit test.
- **`kalshi_get_dashboard_bootstrap`:** Injects `ml_phase3` via `db_pool` (one IPC for markets + ML readiness).
- **`KalshiView`:** Diagnostic strip shows sidecar progress, resolved Kalshi paper rows, next category unlock; Vitest extended.
- Health: cargo check, tsc, **95** lib tests pass; KalshiView tests pass.

## Maintenance notes (2026-06-30, maintenance pass) — Phase 3 category scope

- **`ml_predictor.rs`:** `fetch_category_stats` counts only Kalshi paper rows (`$.ticker` in `full_decision_json`); `KALSHI_TICKER_PREDICATE` constant; LLM header adds Kalshi journal line when Phase 3 incomplete; header test extended.
- **Settings UI:** Clarifies per-category list is Kalshi ticker rows only (mixed `predictions.db` totals unchanged on unified card).
- Health: cargo check, tsc, **94** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 ROADMAP visibility

- **`ml_predictor.rs`:** `phase_3_data_metric_ready`, `kalshi_resolved_predictions` / `kalshi_pending_predictions` on `MLModelStatus`; LLM header shows Phase 3 progress when incomplete; +2 unit tests.
- **Settings UI:** ROADMAP data metric badge; Kalshi-only resolved/pending line vs mixed `predictions.db` totals.
- Health: cargo check, tsc, **94** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 ML prompt + DB hygiene

- **`ml_predictor.rs`:** `format_ml_training_header` (CV ± std, active sidecars) for chat prompts; `DROP INDEX IF EXISTS idx_ml_pred_ticker` on ML table init; +1 unit test.
- **`enhanced_prompt.rs` / `openrouter.rs`:** Shared ML header helper (DRY).
- **Settings UI:** Unified model CV shows ± std when `_meta.json` provides it; TS types extended.
- Health: cargo check, tsc, **92** lib tests pass.

## Maintenance notes (2026-06-29, maintenance pass) — Phase 3 auto-retrain UX

- **`ml_predictor.rs`:** `auto_retrain_eligible` / `resolved_until_auto_retrain` on `MLModelStatus`; helpers + unit test; removed invalid `idx_ml_pred_ticker` index (no `ticker` column on `ml_predictions`).
- **Settings UI:** Shows when auto-retrain after grading is active vs how many resolved rows are still needed (≥10 gate).
- Health: cargo check, tsc, **91** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — Phase 3 category visibility

- **`ml_predictor.rs`:** `ensure_non_sports_sidecar_stats` merges placeholder Politics/Economics/Weather rows when DB has no graded rows in those categories; `nearest_non_sports_sidecar_unlock` + `next_sidecar_*` on `MLModelStatus`; +2 unit tests.
- **Settings UI:** “Next sidecar unlock” line when a target category is still below 10 graded.
- Health: cargo check, tsc, **90** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — Phase 3 progress UX

- **`ml_predictor.rs`:** `trainable_non_sports_categories` / `non_sports_sidecar_target` on `MLModelStatus`; `count_trainable_non_sports_categories` + unit test; debug log when auto-retrain skipped (<10 resolved).
- **Settings UI:** Phase 3 progress line (X/3 Politics/Economics/Weather); clarified ≥10 total graded rows for auto-retrain.
- **`ROADMAP.md`:** Notes Settings tracks the ≥3-category data metric.
- Health: cargo check, tsc, **88** lib tests pass.

## Maintenance notes (2026-06-28, maintenance pass) — P3 ML polish

- **`ml_predictor.rs`:** Auto-retrain after grading only when ≥10 resolved rows (`should_retrain_given_resolved`); passes DB pool from auto-grader; +1 unit test.
- **Settings UI:** Active sidecars show per-model CV accuracy when present in `_meta.json`.
- **`ROADMAP.md`:** Marked code-complete Phase 3 success metrics (sidecar listing, predict routing, CV/mix visibility).
- Health: cargo check, tsc, lib tests.

## Maintenance notes (2026-06-27, maintenance pass) — ML Settings visibility + ROADMAP

- **`ROADMAP.md`:** Created phased plan (P0–P4) with Phase 3 ML success metrics; complements `PRIORITIES.md`.
- **Settings UI:** Auto-retrain helper text; show `trained_at` on unified model card; surface `training_category_breakdown` from `_meta.json`.
- **`ml_predictor.rs`:** `should_retrain_after_grading` gate + unit test.
- Health: cargo check, tsc, lib tests.

## Maintenance notes (2026-06-27, maintenance pass) — P3 ML training loop

- **`ml_predictor.rs`:** `retrain_after_grading` — background unified + sidecar retrain after new grades land.
- **`kalshi/grading.rs`:** Auto-grader spawns ML retrain when `graded > 0` (non-blocking).
- **Settings UI:** **Train unified + sidecar models** + **Refresh status** on ML readiness card (`ml_train_model` IPC).
- Health: cargo check, tsc, **85** lib tests pass.

## Maintenance notes (2026-06-27, maintenance pass) — Kalshi notification prefs wired

- **`kalshi/grading.rs`:** Auto-grader respects `notification_settings.json` — skips win/loss alerts when `kalshi_notifications_enabled` or master `enabled` is off; grading summary gated by `grading_complete_enabled`.
- **`notification.rs`:** Helpers + backward-compatible deserialize for missing `kalshi_notifications_enabled` (defaults on); 3 unit tests.
- **Settings UI:** Toggle **Kalshi market resolved alerts** loads/saves via notification IPC.
- Health: cargo check, tsc, **85** lib tests pass.

## Maintenance notes (2026-06-27, overnight pass) — Kalshi market resolution notifications

- **`notification.rs`:** Added `KalshiMarketWin`, `KalshiMarketLoss` variants to `NotificationType`, `kalshi_notifications_enabled` setting.
- **`kalshi/grading.rs`:** `spawn_auto_grade_task` now accepts AppHandle + DB pool; emits per-prediction Win/Loss notifications and a GradingComplete summary when the auto-grader resolves markets.
- **`lib.rs`:** Passes AppHandle + db_pool to auto-grader.
- You'll now see a notification pop up when a Kalshi paper prediction market resolves (Win ❌ / Loss ✅) with title, ticker, stake, and PnL.
- Health: cargo check, tsc, **82** lib tests pass.

## Maintenance notes (2026-06-26, evening pass) — P3 readiness UX + Phase 4 polish

- **`MLCategoryStats`:** `samples_until_trainable` + `min_resolved_for_sidecar` for Settings progress (e.g. `3/10 graded, 7 more for sidecar`).
- **Dashboard bootstrap:** notes when tape is still the SQLite rehydrate (`showing_persisted_snapshot`) before live refresh completes.
- **`market_cache_store`:** async SQLite roundtrip test for save/load.
- Health: cargo check, tsc, **82** lib tests pass.

## Maintenance notes (2026-06-26, 4pm pass) — Dashboard Phase 4 (SQLite persistence)

- **`kalshi_market_cache` table:** JSON snapshot of last quick/full cache in `predictions.db` (`market_cache_store.rs`).
- **Startup:** `load_persisted_cache` rehydrates `KalshiClient` + `SharedCache` when snapshot age ≤ 24h; API refresh still runs when in-memory TTL (60s) is stale.
- **After fetch:** `store_cache` async-persists to SQLite on every quick/full warm.
- Health: cargo check, tsc, **81** lib tests pass.

## Maintenance notes (2026-06-26, 4pm pass) — Dashboard Phase 4 (partial)

- **Startup quick-cache prefetch:** `lib.rs` spawns `ensure_quick_cache()` immediately on app setup; full catalog warm still runs after 8s idle.
- **Market detail:** 300ms debounce on `computeStakeAdjustment` IPC while editing stake/side (Phase 3 frontend trim).
- Health: cargo check, tsc, **80** lib tests pass, MarketDetailPanel vitest green.

## Maintenance notes (2026-06-26) — Dashboard Phase 2 (shared cache decoupling)

- `Arc<RwLock<Option<KalshiCache>>>` (SharedCache) so cache writes populate both `KalshiClient.cache` + `shared_cache`.
- `FetchInProgressGuard` (AtomicBool) prevents stacked full-catalog warm cycles.
- `kalshi_get_cache_state` Tauri command reads cache state without locking the client mutex.
- `KalshiClient::new()` now accepts `shared_cache: Arc<RwLock<Option<KalshiCache>>>`.
- Managed as Tauri state: `.manage(kalshi_cache_holder)`.
- Health: cargo check, tsc, **80** lib tests pass.
- Committed as `feat(kalshi): decouple KalshiCache into shared Arc<RwLock> for lock-free reads`.

## Maintenance notes (2026-06-23)
- Fixed `unused import: sqlx::sqlite::SqlitePoolOptions` in `src-tauri/src/predictions/tracker.rs` (was test-only; moved use into `#[cfg(test)] mod tests`)
- Verified: cargo check clean, 78 lib tests pass, UI tsc clean.
- P3 items remain blocked pending accumulated graded data in predictions.db
- Working tree was clean at start of pass; no remote configured so no push.

## Maintenance notes (2026-06-24)
- Wired `compute_historical_brier` (from graded Win/Loss predictions in predictions.db), `refresh_historical_brier` Tauri command, and UI trigger in SettingsView.tsx. `VolatilityAdjustedKelly` strategy (with `volatility_adjusted_kelly` fn) now uses real `historical_brier` for auto-shrinkage when graded history exists. (P3 brier support complete; strategy was in prior commit)
- Committed changes from maintenance pass (no remote, skipped push).
- Re-ran health checks post-commit: cargo check, tsc, 78 tests all green.

## Maintenance notes (2026-06-25, evening pass)
- `ml_predictor.py`: trains optional sidecar models for Politics/Economics/Weather when each has 10+ graded samples; `predict_batch` routes to sidecar when present.
- `ml_predictor.rs`: `MLPerCategoryModel` + `per_category_models` on `MLModelStatus` (from `_meta.json` + on-disk joblib check).
- Settings: **ML multi-category readiness** card (`ml_get_model_status`, per-category resolved counts and sidecar status).
- `.gitignore`: `__pycache__` / `*.pyc`.
- Health: cargo check, tsc, **80** lib tests pass.

## Maintenance notes (2026-06-25, afternoon pass)
- Completed Rust wiring for `category_code` on `MLPrediction` (predict JSON + prompt context).
- Python: shared `CATEGORY_MAP`, training `category_breakdown` in `_meta.json` and train response.
- Rust: `MLCategoryStats` + `fetch_category_stats` (SQLite `json_extract` on `full_decision_json`); `MLModelStatus.category_stats` / `training_category_breakdown`; readiness text in status message.
- `enhanced_prompt.rs`: non-sports ML rows show `[cat:N]` when category_code > 0.
- Health: cargo check, tsc, **80** lib tests pass.

## Maintenance notes (2026-06-25)
- Extended `ml_predictor.py` (extract_features_from_db + predict_batch) to support Kalshi predictions: now queries rows with full_decision_json, parses category/fair_probability_pct/edge_points/liquidity etc into category_code + shared numeric features. Sports path unchanged. Enables P3 multi-category ML (politics/econ/weather) once graded Kalshi history accumulates.
- Updated docstring, FEATURE_COLUMNS (added category_code), export, and both train/predict paths.
- Health checks remain green (cargo check, tsc, 78 tests).
- Working tree was clean at start; changes committed below.

---

## High-impact improvements (ranked)

| Priority | Item | Why it matters | Status |
|----------|------|----------------|--------|
| **P0** | Fix grading to use `contract_side` + store `market_price_at_entry` | Unblocks trustworthy paper-sim and the entire calibration loop | ✅ Done |
| **P0** | Background auto-grade for Kalshi (poll resolved markets) | Notifications auto-grade ESPN props only; Kalshi grading was manual | ✅ Done |
| **P1** | Correlated position auto-scaling | Warnings exist (event/series co-exposure) but Kelly stakes were not scaled down | ✅ Done |
| **P1** | Wire `edge_eval` calibrator into Kalshi decision path | Isotonic calibrator applied to `analyze_single_prop` (sports props), not LLM `KalshiTradeDecision` forecasts | ✅ Done |
| **P1** | Kalshi historical price/spread snapshots | `line_tracker.rs` is PrizePicks-only; no candlestick API in `kalshi/client.rs` — blocks CLV tracking and momentum signals | ✅ Done |
| **P1** | Kalshi-native correlation engine | `correlation.rs` is NFL prop families; portfolio checks were ticker-prefix heuristics. Now a native correlation cluster graph links distinct series by shared macro/political driver | ✅ Done |
| **P2** | Persist `localMaxBetPct` to config | Now a persisted `max_bet_pct` config field, read/written by SettingsView + MarketDetailPanel | ✅ Done |
| **P2** | Sync bankroll limits from `predictions.db` + paper positions | Makes daily/weekly cap warnings and `BankrollView` accurate | ✅ Done |
| **P2** | Model disagreement flags at entry | Flag when `fair_probability_pct` diverges sharply from market implied prob at decision time | ✅ Done |
| **P2** | CLV per prediction | Grading records close price and CLV on paper predictions | ✅ Done |
| **P3** | Volatility-adjusted Kelly from historical Brier | Shrinkage slider is manual; handoffs call for Brier-driven auto-shrinkage | ✅ Done (2026-06-24; brier compute/refresh/strategy wired) |
| **P3** | Multi-category ML classifiers (politics/econ/weather) | Current ML is scikit-learn on sports prop features via Python subprocess; README still lists ML training as unchecked | ⬜ In progress (2026-06-27; auto-retrain after Kalshi auto-grade + Settings train button; sidecar trainers when 10+ graded/category; awaits graded Kalshi history) |

---

## Remaining count

| Tier | Done | Remaining |
|------|------|-----------|
| P0 | 2 | **0** |
| P1 | 4 | **0** |
| P2 | 4 | **0** |
| P3 | 1 | **1** |

**1 item left** (Multi-category ML classifiers — now in progress). VolatilityAdjustedKelly brier support shipped 2026-06-24. Plus the off-roadmap notification-settings persistence fix (now shipped).

---

## P0 implementation notes (shipped)

- `src-tauri/src/kalshi/grading.rs` — contract-side grading, binary PnL, `grade_pending_predictions`, `spawn_auto_grade_task`
- `src-tauri/src/kalshi/models.rs` — `contract_side`, `market_price_at_entry` on predictions
- `src-tauri/src/predictions/tracker.rs` — rich `KalshiTradeDecision` extraction
- `src-tauri/src/lib.rs` — auto-grade task on startup

---

## P2 implementation notes (shipped)

- `src-tauri/src/bankroll.rs` — async `get_bankroll_summary_synced`, `apply_bankroll_cap`, prediction/paper exposure aggregation
- `src-tauri/src/commands/mod.rs` — bankroll-aware stake adjustment and paper decision capping
- UI: `src-ui/src/components/SettingsView.tsx`, `src-ui/src/components/KalshiPredictionsPanel.tsx`
- `src-tauri/src/config.rs` — `max_bet_pct` persisted config field (resolves the `localMaxBetPct` item); `MarketDetailPanel.tsx` writes it via config save

**P2 remaining:** none.

---

## P1 implementation notes (shipped)

- `src-tauri/src/kalshi/portfolio_risk.rs` — Kelly scaling (event 0.50, series 0.75, **cluster 0.82**, category 0.90, same-ticker 0.85)
- `src-tauri/src/analysis/calibration.rs` — isotonic calibrator wired into Kalshi paper trades
- `src-tauri/src/kalshi/price_tracker.rs` — snapshots on `kalshi_refresh`, `kalshi_get_price_history`
- UI: `src-ui/src/components/KalshiView.tsx`, `MarketDetailPanel.tsx`, `KalshiPredictionsPanel.tsx`, `PriceHistoryChart.tsx`

**P1 native correlation graph (shipped 2026-06-22):** `CorrelationStrength::Cluster` + `CORRELATION_CLUSTERS` map in `portfolio_risk.rs` links distinct series sharing a macro/political driver (`us-rates-inflation`: CPI/PCE/Fed/payrolls/GDP; `us-federal-politics`: president/senate/house/party-control). Conflict explanations name the driver. The cluster map is the extension point for future event-graph edges.

---

## Suggested next target: P3

P0–P2 are complete. 

1. Volatility-adjusted Kelly from historical Brier (auto-shrinkage) — ✅ Done (2026-06-24; `volatility_adjusted_kelly` fn + `compute_historical_brier` + `refresh_historical_brier` command + UI trigger wired; strategy now uses real data for shrinkage when graded history accumulates in predictions.db)
2. Multi-category ML classifiers (politics/econ/weather) — ⬜ In progress (2026-06-25; sidecar train/infer wired; UI readiness in Settings; fully active once politics/econ/weather each accumulate 10+ graded rows)

Off-roadmap fix shipped 2026-06-22: notification settings now persist to `~/.openclaw/kalshi-monster/notification_settings.json` (`notification::load_settings`/`save_settings`); previously `save_notification_settings` only logged and `get_notification_settings` always returned defaults.

---

## Dashboard performance (deferred)

**Phase 1 (shipped 2026-06-17):** flat `GET /markets` quick cache (replaces nested `/events` for dashboard load). See `kalshi/client.rs` — `fetch_markets_flat_pages`, `ensure_quick_cache`.

### Phase 2 — Decouple cache reads from long fetches ✅ Done (2026-06-26)

- Extract `Arc<RwLock<KalshiCache>>` + `fetch_in_progress` guard so UI reads never block on 20-page full warm ✅
- Background full-catalog warm writes cache without holding the outer `KalshiClient` mutex across HTTP pagination ✅
- Add `kalshi_get_cache_state` Tauri command (read-only, no client lock) ✅
- Optionally slim cache to `KalshiMarketSummary` instead of full `KalshiMarket`
- **Target:** warm revisit under 300ms; category switch under 500ms

### Phase 3 — Frontend critical-path trim (shipped 2026-06-23)

- Keep `KalshiView` mounted across tab switches (avoid cold reload)
- Combined IPC: `kalshi_get_dashboard_bootstrap` → `{ markets, categories, cache_full }` ✅ Shipped
- Show partial-cache indicator when `full_catalog == false` ✅ Shipped (cacheLabel/partialCatalog in KalshiView)
- Defer `KalshiPredictionsPanel` load; debounce `computeStakeAdjustment` in market detail ✅ Shipped (predictions deferred via `marketsReady`; stake debounce 300ms in MarketDetailPanel)
- Calibration status inline display in MarketDetailPanel ✅ Shipped

### Phase 4 — Startup prefetch and persistence (optional)

- Prefetch quick cache at app startup (before user opens dashboard) ✅ Shipped (2026-06-26)
- Delay full warm until quick cache exists + idle window (or explicit Refresh only) ✅ (quick prefetch + 8s delayed full warm)
- Persist summary cache to SQLite for instant next-launch paint ✅ Shipped (2026-06-26; `kalshi_market_cache` + startup rehydrate)

---

## Environment notes

- Canonical WSL repo (`~/.openclaw/agents/coderclaw/workspace/kalshi-monster`) was unreachable as of 2026-06-17
- `edge-eval` lives at `C:\\Users\\ethan\\kalshi-build\\edge-eval` (sibling path); prop calibrator inlined in `kalshi-monster/src-tauri/src/analysis/`
