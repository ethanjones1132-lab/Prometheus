# Project Recovery Hunt — Findings Report

**Scope:** Reconstruct what happened to the lost project and identify every recoverable copy of source code, logs, and build artifacts across the tools named: Antigravity, Codex, Claude, Hermes Desktop, OpenCode.

**Current workspace:** `C:\Users\ethan\kalshi-build`

---

## 1. Executive summary

The original source tree lived inside the **WSL Ubuntu** filesystem at:

```
/home/ethan/.openclaw/agents/coderclaw/workspace/kalshi-monster
/home/ethan/.openclaw/agents/coderclaw/workspace/prizepicks-monster
```

That WSL distro has been **unregistered**, so the original working copies are gone.

What remains on Windows:

| Item | Location | State |
|------|----------|-------|
| **Primary recovery tree** | `C:\Users\ethan\kalshi-build` | Active, partially reconstructed; has the most complete Rust source for Kalshi Monster and the shared `edge-eval` / `monster-edge-core` crates. |
| **Older desktop snapshot** | `C:\Users\ethan\Desktop\kalshi-monster` | 10,725 files (mostly `node_modules`, `target`, `dist`); source is an *earlier* version than `kalshi-build` (48 source files vs 88). |
| **Runtime data** | `C:\Users\ethan\.openclaw\kalshi-monster` | Only 21 files: config, bankroll, SQLite DB, session JSONs. Not source. |
| **Runtime data** | `C:\Users\ethan\.openclaw\prizepicks-monster` | Only 27 files: same kind of runtime data. |
| **Tauri WebView2 profiles** | `C:\Users\ethan\AppData\Local\com.kalshi.monster` (44 MB)<br>`C:\Users\ethan\AppData\Local\com.prizepicks.monster` (43 MB) | Browser cache/history/cookies only. No source or app data. |
| **Built Windows EXEs** | Desktop + OneDrive Desktop | `Kalshi Monster.exe`, `KalshiMonster.exe`, `PrizePicks Monster.exe`, `PrizePicksMonster.exe` — working artifacts, not source. |
| **Claude session logs** | `~\.claude\projects\--wsl-localhost-ubuntu-home-ethan--openclaw-agents-coderclaw-workspace-kalshi-monster\` | 688 lines of exploration/planning logs from the WSL source. Contain snippets of original files but no bulk source. |
| **Claude session logs** | `~\.claude\projects\--wsl-localhost-ubuntu-home-ethan--openclaw-agents-coderclaw-workspace-prizepicks-monster\` | 1,969 lines of logs. Source likely reconstructible from Read results if needed. |
| **Claude plans** | `~\.claude\plans\` | Six detailed Markdown plans describing the intended architecture, gaps, and next steps for Kalshi Monster, PrizePicks Monster, and the shared edge-validation core. |
| **Codex rollout summaries** | `~\.codex\memories\rollout_summaries\` | Three markdown summaries documenting successful Windows release builds from the WSL source in June 2026. |
| **Codex/Gemini/OpenCode/Hermes/Antigravity** | Various caches | No full source copies found; only references, build artifacts, and runtime config. |

**Bottom line:** The best available source is the current `kalshi-build` tree. The Desktop copy is an older snapshot and should not overwrite the current tree. PrizePicks Monster source is effectively lost except for what can be mined from Claude logs and build artifacts.

---

## 2. Current workspace (`kalshi-build`) inventory

```
kalshi-build/
├── edge-eval/                  # Shared evaluation crate
│   ├── Cargo.toml
│   └── src/
│       ├── calibration.rs
│       ├── lib.rs
│       └── types.rs
├── kalshi-monster/             # Tauri + React app
│   ├── README.md
│   ├── AGENTS.md
│   ├── PRIORITIES.md
│   ├── reports/                # Backtest/calibration JSON + Markdown
│   ├── scripts/                # launch.sh, tauri-dev.bat, tauri-dev.sh
│   ├── src-tauri/
│   │   ├── Cargo.toml
│   │   ├── tauri.conf.json
│   │   ├── capabilities/
│   │   ├── icons/
│   │   └── src/                # 88 source files incl. analysis/, predictions/, chat/, kalshi/, etc.
│   └── src-ui/
│       ├── package.json
│       ├── vite.config.ts
│       └── src/                # React/TypeScript UI
├── monster-edge-core/          # Shared edge math crate
│   ├── Cargo.toml
│   └── src/
│       ├── calibrator.json
│       └── lib.rs
└── RECOVERY_REPORT.md          # This file
```

### Rust source in `kalshi-build/kalshi-monster/src-tauri/src` (88 files)

- `analysis/` — calibration, context, edge_calculator, matchup_analyzer, mod, parlay_correlation, prop_scorer
- `chat/` — decision_schema, enhanced_prompt, kalshi_context, mod, openrouter, session
- `commands/mod.rs`
- `kalshi/` — client, grading, mod, models, portfolio_risk, price_tracker
- `predictions/` — grading, mod, storage, tracker
- `football/` — API client, data, injector, live_data, mod, player_stats
- Standalone: bankroll, bot, config, correlation, error, eval_adapter, lib, line_tracker, main, ml_predictor, notification, prizepicks, weather

### React/TypeScript UI in `kalshi-build/kalshi-monster/src-ui/src`

- `App.tsx`, `main.tsx`, `index.css`
- `components/` — ChatView, KalshiPredictionsPanel, KalshiView, MarketDetailPanel, PriceHistoryChart, PropsView
- `hooks/useChat.ts`
- `services/kalshi.ts`, `services/tauri.ts`
- `types/index.ts`, `types/kalshi.ts`

### Shared crates

- `edge-eval` — evaluation math (calibration, backtest, ROI, recalibrate)
- `monster-edge-core` — shared edge math used by both Monster apps

---

## 3. Comparison: `kalshi-build` vs. `Desktop\kalshi-monster`

The Desktop copy is **older/different** and should **not** be treated as the master source.

| Feature | `kalshi-build/kalshi-monster` | `Desktop/kalshi-monster` |
|---------|-------------------------------|--------------------------|
| Source files (excl. deps/build) | 88 | 48 |
| `analysis/` module | ✅ Yes | ❌ No |
| `predictions/` module | ✅ Yes | ❌ No |
| `eval_adapter.rs` | ✅ Yes | ❌ No |
| `chat/` split into files | ✅ Yes (`mod.rs`, `enhanced_prompt.rs`, etc.) | ❌ No (`chat.rs`, `chat/openrouter.rs`) |
| `kalshi/` models & grading | ✅ Yes | ❌ Minimal (`kalshi/mod.rs`, `client.rs`, `types.rs`) |
| `bankroll/` subdir | ❌ No | ✅ Yes |
| `db/` subdir | ❌ No | ✅ Yes |
| `props/` subdir | ❌ No | ✅ Yes |
| `portfolio/` subdir | ❌ No | ✅ Yes |
| `notifications/` subdir | ❌ No | ✅ Yes |
| `export/` subdir | ❌ No | ✅ Yes |
| `market_data/` subdir | ❌ No | ✅ Yes |
| `ml/` subdir | ❌ No | ✅ Yes |
| Shared `edge-eval` dependency | ✅ Yes | ❌ No |
| Shared `monster-edge-core` dependency | ✅ Yes | ❌ No |

**Interpretation:** The current `kalshi-build` tree represents a later refactoring that split the shared evaluation math into `edge-eval` and `monster-edge-core`, and added the `analysis/`, `predictions/`, and `eval_adapter` layers. The Desktop snapshot predates that refactor.

---

## 4. What is missing or incomplete

Based on the Claude plans (`~\.claude\plans\alot-of-work-has-kind-yao.md`) and the current source:

### High-priority gaps in Kalshi Monster

1. **Paper trading is a shim, not a simulator**
   - No `paper_account`, `paper_lots`, or `paper_equity_snapshots` tables.
   - No `paper_place_trade`, `paper_close_position`, `paper_get_positions`, `paper_settle_pending`, `paper_get_analytics` commands.
   - P&L math uses `±stake_amount` instead of correct Kalshi payout `qty*(100−entry)/100`.
   - Paper grading is not decoupled from real grading.

2. **Missing UI components**
   - `PaperTradeTicket.tsx` does not exist.
   - `PaperSimView.tsx` exists but needs rebuild on the glass UI kit.
   - `KalshiView.tsx` lacks per-row "Paper trade" button.

3. **Dead legacy commands**
   - Six sports-prop commands (`analyze_prop`, `analyze_multiple_props`, etc.) are implemented but unregistered and unused.

4. **Bot config persistence**
   - TODO in `commands/mod.rs` around line 2144.

5. **Structured-output reliability**
   - `KalshiTradeDecision` JSON extraction from chat needs hardening.

### PrizePicks Monster

- **No source copy found on Windows.**
- Only runtime data, build artifacts, and Claude session logs remain.
- The `~\.gemini\history\prizepicks-monster` git repo has only an empty initial commit.
- Recovery would require mining the 1,969-line Claude log or restoring from backup/WSL.

---

## 5. Tool-by-tool search results

### Antigravity
- Only VS Code extension files and Godot-related extensions.
- No project source found.

### Codex
- `~\.codex\memories\rollout_summaries\` has three build reports confirming the WSL source location and successful Windows builds.
- Sandbox logs contain grep commands referencing `kalshi-monster` and `prizepicks-monster` but no source files.
- No recoverable source.

### Claude
- **Most valuable logs.**
- `~\.claude\plans\` has detailed implementation plans.
- `~\.claude\projects\--wsl-localhost-ubuntu-home-ethan--openclaw-agents-coderclaw-workspace-kalshi-monster\` has logs from the original WSL workspace.
- `~\.claude\projects\--wsl-localhost-ubuntu-home-ethan--openclaw-agents-coderclaw-workspace-prizepicks-monster\` has logs from the original PrizePicks workspace.
- `~\.claude\projects\C--Users-ethan-kalshi-build\` has a recent log of Claude exploring the current recovery tree.

### Hermes Desktop
- `~\.hermes\.env` only.
- No source.

### OpenCode
- `~\.config\opencode\` and `~\.local\state\opencode\` contain config only.
- No source.

### Windows AppData runtime directories
- `C:\Users\ethan\AppData\Local\com.kalshi.monster` (44 MB) and `C:\Users\ethan\AppData\Local\com.prizepicks.monster` (43 MB) exist.
- Both contain only **Tauri WebView2 / Edge browser profiles** (`EBWebView`): cache, cookies, history, browser metrics, etc.
- No application source code, no app-specific SQLite databases, and no recoverable prediction/trading data.
- Not useful for source recovery.

---

## 6. Recoverable artifacts

### Immediate recovery value

1. **`C:\Users\ethan\kalshi-build`** — this is the best source tree. Keep it as the master.
2. **`C:\Users\ethan\Desktop\kalshi-monster`** — can be used to cherry-pick older modules (`bankroll/`, `db/`, `props/`, `portfolio/`, `notifications/`, `export/`, `market_data/`, `ml/`) if they are still relevant, but only after verifying they match the current architecture.
3. **Claude plans** — provide the exact roadmap to finish the project.
4. **Built EXEs** — can be run to understand behavior, but cannot restore source.

### Not recoverable without external action

- Original WSL source trees (`kalshi-monster` and `prizepicks-monster`).
- PrizePicks Monster source (must be reconstructed from Claude logs or a separate backup).

---

## 7. Recommended next steps

1. **Protect the current `kalshi-build` tree.** Do not let any older copy overwrite it.
2. **Initialize git** in `kalshi-build` and commit the current state as a baseline.
3. **Use the Claude plans** (`alot-of-work-has-kind-yao.md`, `do-not-make-any-glittery-hare.md`, etc.) as the authoritative roadmap.
4. **Decide on PrizePicks Monster:**
   - If still needed, attempt source reconstruction from the Claude PrizePicks logs, or
   - Treat it as out-of-scope and focus on Kalshi Monster + shared crates.
5. **Implement the paper-trading engine** as the highest-value missing piece (Phase 1 of `alot-of-work-has-kind-yao.md`).
6. **Run `cargo test` and `npm run build`** in the current tree to identify compile/runtime blockers before making large changes.

---

*Report generated by Kimi Code CLI during recovery hunt.*
