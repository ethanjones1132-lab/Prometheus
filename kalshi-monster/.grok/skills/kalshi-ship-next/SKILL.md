---
name: kalshi-ship-next
description: >
  Pick the highest-leverage incomplete item from PRIORITIES.md, implement it
  end-to-end (backend + UI when needed), verify, and update the roadmap.
  Use when asked to "ship next", "/kalshi-ship-next", "work on next priority",
  "continue the roadmap", or "implement P2". Runs kalshi-maintain before claiming done.
metadata:
  short-description: "Ship next PRIORITIES.md item"
---

# kalshi-ship-next — Agent implementation loop

Autonomously ship the next backlog item for **kalshi-monster**.

Working copy: `C:\Users\ethan\kalshi-build\kalshi-monster`

## Step 1 — Select target

1. Read `PRIORITIES.md` fully.
2. Pick **one** item using this order:
   - **Perf Phase 2** if user complained about slowness and Phase 1 is shipped
   - **Suggested next target** section (currently P2 bankroll sync or CLV)
   - Else lowest incomplete tier: P1 partial → P2 → P3 → perf Phases 2–4
3. Tell the user which item you chose and why (one paragraph).

If the user named a specific item, that overrides the picker.

## Step 2 — Scope and plan

Before coding:

- Confirm product surface is **Kalshi** (not PrizePicks) unless item is shared infra
- Read relevant existing modules; extend don't rewrite
- Note files you expect to touch
- For UI work: match `KalshiView` / `index.css` patterns

Do not expand scope beyond the single priority row.

## Step 3 — Implement

Follow repo rules from `AGENTS.md`:

- Research/analytics posture — no real order execution
- Tauri v2 IPC: camelCase from JS (`contractSide`, `sessionId`)
- Prefer `cargo test kalshi::` for backend verification
- UI: `npm run typecheck` and `npm run build` when touching `src-ui/`

Use the global `implement` or `check-work` skills when the change is large or risky.

## Step 4 — Verify

1. `.\scripts\agent-healthcheck.ps1` — must PASS
2. If you added commands or DB fields, smoke-test the invoke path in code review
3. Run `/check-work` or invoke `check-work` skill for non-trivial diffs

## Step 5 — Update PRIORITIES.md

- Set status to ✅ Done or ⚠️ Partial with one-line note
- Update "Remaining count" table
- Update "Last updated" date
- Add a short "implementation notes" bullet under the tier section if new

## Step 6 — Handoff summary

```
## Shipped: <item name>
- Tier: P?
- Files: ...
- Verification: healthcheck PASS, tests ...
- Remaining: <count from PRIORITIES.md>
- Next suggested: ...
```

## Default next targets (as of roadmap)

When no user preference:

1. P2 — Sync bankroll limits from `predictions.db` + paper positions
2. P2 — CLV per prediction (entry vs close)
3. Perf Phase 2 — RwLock cache + non-blocking full warm

## Do not

- Ship multiple P2 items in one invocation unless user asks
- Mark P1 correlation "Done" without event-graph logic
- Skip healthcheck before claiming complete