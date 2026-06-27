# AGENTS.md — Monster App Shared Repo Working Rules

This file gives autonomous AI agents the minimal project-specific context needed to work safely in this repo.

## What this repo is
A market-app codebase currently branded around Kalshi Monster, with shared infrastructure and some product-specific surfaces that may overlap with other Monster offers.

## Read first
- `README.md` — current repo overview and calibration posture
- `PRIORITIES.md` — ranked P0–P3 improvement backlog and completion status
- `ROADMAP.md` — phased plan and Phase 3 ML success metrics

## Agent automations (for AI agents, not user CI)

Project skills live in `.grok/skills/`. Invoke them when maintaining or shipping work without the user repeating instructions.

| Skill | Trigger | What it does |
|-------|---------|--------------|
| `kalshi-maintain` | `/kalshi-maintain`, "health check", session start with no task | Runs `scripts/agent-healthcheck.ps1`, reconciles `PRIORITIES.md`, checks perf path |
| `kalshi-ship-next` | `/kalshi-ship-next`, "ship next priority", "continue roadmap" | Picks next incomplete item from `PRIORITIES.md`, implements, verifies, updates roadmap |

**Healthcheck script** (run from repo root):

```powershell
.\scripts\agent-healthcheck.ps1
```

Checks: UI typecheck, `cargo check`, `cargo test kalshi::`, `PRIORITIES.md` present, paper module wired (`pub mod paper`, `init_paper_tables`), flat `/markets` quick-cache path in `client.rs`.

**Paper journal** — `src-tauri/src/paper/mod.rs` tracks cash equity separately from prediction log rows. IPC: `paper_get_analytics`, `paper_get_positions`, `paper_settle_pending`, `paper_reset_account`. `kalshi_record_paper_decision` opens a paper lot when stake > 0.

**Proactive behavior:** After Kalshi changes, run `kalshi-maintain` before claiming done. When the user wants forward progress on the backlog, use `kalshi-ship-next`.

## Key areas
- `src-tauri/` — app code
- `reports/` — evaluation and calibration artifacts
- `scripts/` — helper scripts

## Important caveat
Tasks may target **Kalshi Monster** or **PrizePicks Monster**. Do not assume every file or conclusion is product-specific just because it lives in this repo.

## Working rules
1. Identify which product surface the task targets before making conclusions.
2. Label shared infrastructure vs product-specific behavior explicitly.
3. Preserve the repo's research / analytics posture; do not imply real order execution.
4. Prefer real reports, code evidence, and verified behavior over naming assumptions.
5. If evidence for a product-specific claim is thin, say so clearly.
