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

**Status (2026-07-14 morning):** 🟡 Root cause fixed (dual-runtime blocking RwLock on cache write, KB-1 commits `bead675`–`340928f`). Code fixes shipped: async cache write, empty-tape hints, config sync on bootstrap, credential-aware data-quality notes, tape-count pinning, sidecar dry-run packaging. **Remaining blocker: live credential verification on user machine** — not automatable in a cron pass. All tests green (222 lib + 40 vitest).

---

## KB-2 — Analyst page needs major work (P0 UX)

**Symptom:** **Analyst** tab (`ChatView.tsx`) was minimal vs product goals — inline styles, no session sidebar polish, weak market-context affordances when chat is blind to live tape.

**Scope (one pass = one slice):**

1. **Layout / design system** — Replace ad-hoc inline styles with shared app tokens; header, message list, composer parity with Markets/Settings. ✅ **Done** (2026-07-09, KB-2a).
2. **Market context UX** — Show active ticker / bootstrap snippet when user arrives from **Analyze** on Markets; surface when `build_kalshi_context` / Fincept context failed (degraded banner). ✅ **Done** (2026-07-09, KB-2a — structured `KalshiChatContextStatus` + `chat-kalshi-context` event + `kalshi_get_chat_context_status` polling command + amber banner in UI).
3. **Sessions** — Visible session list, rename/delete, empty state that points to Markets when tape is cold (links KB-1). ✅ **Done** (2026-07-13, KB-2b — `rename_session()` backend + `rename_chat_session` Tauri command + inline rename UI in ChatView).
4. **Streaming** — Clear streaming indicator, cancel, error retry; optional quick prompts tied to **live** categories (not generic placeholders when tape empty). ✅ **Done** (livePrompts from categories, streaming indicator with Stop button, retry from lastFailedPrompt, error banner, streamCaret).
5. **Paper / forecast hooks** — From analyst answer, one-click **record paper decision** where stake/verdict already discussed (plan Phase 0 ledger). ✅ **Done** (paperFromChat utility with JSON + heuristic extraction, unit normalization, Kelly caps; Record button in MessageBubble; paperBusy/paperMsg feedback; `onOpenPaper` callback from App.tsx; 4 vitest tests in paperFromChat.test.ts).

**Fix targets:** `src-ui/src/components/ChatView.tsx`, `src-ui/src/hooks/useChat.ts`, `src-ui/src/App.tsx`, `src-tauri/src/commands/mod.rs` (chat IPC only if needed for context errors).

**Acceptance:** Analyst usable for a full thread with visible context + errors; vitest for `initialPrompt` from Markets; no regression on `send_message_stream`. ✅ **All complete** — ChatView has 2 vitest tests (sessions + context pinning); paperFromChat has 4 tests; streaming + errors + retry all wired.

**Status (2026-07-14 morning): ✅ Complete.** All 5 slices (KB-2a through KB-2e/d) are shipped. No remaining UX items.

---

## Suggested fleet sequence

1. **KB-1** — Restore market population (blocking). → 🟡 Root cause fixed; awaiting live credential verification.
2. **KB-2a** — Context + error banners when tape missing. → ✅ Done
3. **KB-2b** — Layout / sessions. → ✅ Done
4. **KB-2c** — Paper hook from chat. → ✅ Done
5. **KB-2d/e** — Streaming + quick prompts. → ✅ Done

**Next:** Resume Phase 1 items in the integration plan when KB-1 live verification confirms acceptance.

**Last updated by maintenance pass:** 2026-07-19 midday cron — KB-1 🟡; calibration **105/200** (+18 Jul-18 city-high resolves); logged +25 short-horizon forecasts (ledger 218/105/113); health 268 lib green.
