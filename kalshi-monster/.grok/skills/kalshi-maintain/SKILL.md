---
name: kalshi-maintain
description: >
  Autonomous maintenance loop for kalshi-monster: run healthcheck, reconcile
  PRIORITIES.md against code, spot perf regressions, fix trivial breakage.
  Use when asked to "maintain kalshi", "health check", "/kalshi-maintain",
  "keep the codebase healthy", or when starting a session on this repo with
  no specific feature task. Also use proactively after large Kalshi changes.
metadata:
  short-description: "Healthcheck and reconcile kalshi-monster"
---

# kalshi-maintain — Agent maintenance loop

You are maintaining **kalshi-monster** for the user. This is an agent-side automation — no user-facing CI required.

Working copy: `C:\Users\ethan\kalshi-build\kalshi-monster`

## When to run

- User says maintain / health check / keep codebase healthy
- After shipping Kalshi backend or UI changes (before claiming done)
- At session start when the user has no specific task but is in this repo
- When PRIORITIES.md may be stale after recent work

## Steps

### 1. Run the healthcheck script

From repo root (PowerShell):

```powershell
.\scripts\agent-healthcheck.ps1
```

If it fails, fix the failing step before continuing. Re-run until PASS.

### 2. Reconcile PRIORITIES.md

Read `PRIORITIES.md` and verify status claims against code:

| Claim | Where to verify |
|-------|-----------------|
| P0 grading / auto-grade | `src-tauri/src/kalshi/grading.rs`, `lib.rs` spawn |
| P1 portfolio Kelly | `src-tauri/src/kalshi/portfolio_risk.rs` |
| P1 isotonic calibrator | `src-tauri/src/analysis/calibration.rs` |
| P1 price snapshots | `src-tauri/src/kalshi/price_tracker.rs` |
| P1 correlation partial | ticker-prefix heuristics only — no event-graph engine |
| Phase 1 perf | `client.rs` → `fetch_markets_flat_pages`, `ensure_quick_cache` |
| Paper journal | `lib.rs` → `pub mod paper`, `init_paper_tables`; `paper_get_analytics` registered |
| P2/P3 not started | grep for expected symbols; if implemented, update table |

Update `PRIORITIES.md` **only if** you find a mismatch (wrong status, stale date, wrong remaining count). Keep edits minimal.

### 3. Perf spot-check (Kalshi dashboard path)

Confirm quick-load still uses flat markets, not nested events:

- `ensure_quick_cache()` must call `fetch_markets_flat_pages`, not `fetch_events_catalog_from_base`
- Full warm may still use nested `/events` — that's OK for background only
- `KalshiView.tsx`: search on Enter/button, sequential load, request-id guard

If regression found, fix or file under "Dashboard performance" in PRIORITIES.md.

### 4. Report

Post a short maintenance report:

```
## kalshi-maintain — <date>
- Healthcheck: PASS/FAIL
- PRIORITIES sync: unchanged / updated (what changed)
- Perf path: OK / issue (detail)
- Suggested next: <top item from PRIORITIES.md>
```

### 5. Optional auto-fix scope

Fix without asking only if:

- Typecheck or `cargo test kalshi::` failure from your recent edits
- Obvious typo or broken import you introduced this session
- PRIORITIES.md date/count drift

Do **not** start P2/P3 feature work unless the user asks or invokes `kalshi-ship-next`.

## References

- `AGENTS.md` — product boundaries and rules
- `PRIORITIES.md` — backlog source of truth
- `scripts/agent-healthcheck.ps1` — automated checks