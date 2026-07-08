# Kalshi Monster — Fleet backlog (2026-07-08)

User-reported product weaknesses. **Cron passes implement one KB-* item per run** until closed; KB-* **preempts Phase 1+ plan work** (World Markets, sidecar expansion) until markets populate reliably and Analyst is usable.

Canonical copy also lives in `C:\Users\ethan\OneDrive\Desktop\kalshi-monster-fincept-integration-plan.md` (Section 13, items 6–7).

## KB-1 — Kalshi markets not populating in UI (P0)

**Symptom:** Markets board stays empty, thin, or stuck warming; user suspects **tokio** / background tasks failing.

**User-visible path:** `KalshiView.tsx` → `kalshiApi.getDashboardBootstrap()` (category **All**) → `kalshi_get_dashboard_bootstrap` → `KalshiClient` cache (`ensure_quick_cache`, `fetch_all_markets`).

**Likely failure modes (investigate in order):**

1. **Dual async runtime** — `lib.rs` builds a standalone `tokio::Runtime` for DB init, while Tauri commands use `tauri::async_runtime`. Any bare `tokio::spawn` (e.g. `kalshi/client.rs` `schedule_persist` ~514) without the Tauri runtime handle may not run or may panic when no reactor is current.
2. **Startup warm tasks** — `lib.rs` `tauri::async_runtime::spawn` for `ensure_quick_cache` and delayed `fetch_all_markets`; failures only log as `tracing::warn` — UI may show empty tape with no error.
3. **Auth / API** — Kalshi JWT or base URL; bootstrap should surface `data_quality_notes` and IPC errors to `KalshiView` `error` state.
4. **Stale persisted cache** — SQLite hydrate with zero markets; UI shows `market_count: 0` without forcing refresh CTA.

**Fix targets:**

| Area | Files |
|------|--------|
| Runtime / spawn | `src-tauri/src/lib.rs`, `src-tauri/src/kalshi/client.rs`, `src-tauri/src/kalshi/grading.rs` |
| Bootstrap IPC | `src-tauri/src/commands/mod.rs` (`kalshi_get_dashboard_bootstrap`) |
| UI empty / error | `src-ui/src/components/KalshiView.tsx`, `src-ui/src/services/kalshi.ts` |

**Acceptance:** With valid Kalshi credentials, opening **Markets** shows ≥ `INITIAL_MARKET_LIMIT` rows within one refresh cycle; `market_count` > 0; bootstrap errors visible in UI; `cargo test` + KalshiView vitest green.

**2026-07-08 cron slice (tag `kb1-tape-1`):** `tauri::async_runtime::spawn` for cache persist; empty-cache refetch in `ensure_quick_cache`; bootstrap + UI empty-tape alert/retry; tests extended. Live credential verification still required to close KB-1.

**Tests to add/extend:** Lib test for bootstrap with mock cache; vitest for empty bootstrap → error banner + retry. ✅ vitest added; lib `data_quality_notes` extended.

---

## KB-2 — Analyst page needs major work (P0 UX)

**Symptom:** **Analyst** tab (`ChatView.tsx`) is minimal vs product goals — inline styles, no session sidebar polish, weak market-context affordances when chat is blind to live tape.

**Scope (one pass = one slice):**

1. **Layout / design system** — Replace ad-hoc inline styles with shared app tokens; header, message list, composer parity with Markets/Settings.
2. **Market context UX** — Show active ticker / bootstrap snippet when user arrives from **Analyze** on Markets; surface when `build_kalshi_context` / Fincept context failed (degraded banner).
3. **Sessions** — Visible session list, rename/delete, empty state that points to Markets when tape is cold (links KB-1).
4. **Streaming** — Clear streaming indicator, cancel, error retry; optional quick prompts tied to **live** categories (not generic placeholders when tape empty).
5. **Paper / forecast hooks** — From analyst answer, one-click **record paper decision** where stake/verdict already discussed (plan Phase 0 ledger).

**Fix targets:** `src-ui/src/components/ChatView.tsx`, `src-ui/src/hooks/useChat.ts`, `src-ui/src/App.tsx`, `src-tauri/src/commands/mod.rs` (chat IPC only if needed for context errors).

**Acceptance:** Analyst usable for a full thread with visible context + errors; vitest for `initialPrompt` from Markets; no regression on `send_message_stream`.

---

## Suggested fleet sequence

1. **KB-1** — Restore market population (blocking).
2. **KB-2a** — Context + error banners when tape missing.
3. **KB-2b** — Layout / sessions.
4. **KB-2c** — Paper hook from chat.

Then resume Phase 1 items in the integration plan.