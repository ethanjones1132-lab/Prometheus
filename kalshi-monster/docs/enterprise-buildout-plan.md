# Enterprise-Grade Kalshi Monster Build-Out Plan

## Summary

Kalshi Monster should feel like a focused, professional prediction-market intelligence desktop app: fast to open, Kalshi-first, secure with local secrets, calm in the UI, and clear about paper trading versus real execution.

This plan preserves the local-first research/paper-trading posture. It does not add real-money order placement or automatic execution.

## Implementation Streams

### 1. Product Focus and Information Architecture

- Open to the Kalshi markets dashboard by default.
- Remove the sports prop demo from primary navigation.
- Keep sports tooling available only as explicit secondary/advanced functionality.
- Rename navigation around the core jobs: Markets, Analyst, Paper Trades, Settings.
- Replace player-prop onboarding copy with Kalshi event-contract language.
- Ensure empty states and docs point users toward searching Kalshi markets, opening market detail, asking the analyst, and recording paper decisions.

### 2. Market Dashboard and UX Polish

- Add a dashboard bootstrap path that returns top markets, category stats, cache status, and freshness metadata together.
- Show clear data state: live/cached, partial/full catalog, cache age, refresh in progress, and errors.
- Make market cards scannable: ticker, category, title, YES probability, spread, 24h volume, liquidity, close time, and status.
- Keep the dashboard dense and calm rather than hero-led or demo-led.
- Preserve search and category filtering without reloading on every keystroke.

### 3. Market Detail and Decision Ticket

- Turn market detail into a decision workbench with market mechanics, price history, risk flags, and a paper trade ticket.
- Show settlement-sensitive cues: status, close time, expiration, early-close flag, provisional flag, spread, liquidity, and current side price.
- Make decision actions explicit: Record YES, Record NO, Watch, and Pass.
- Guard paper entries when price is stale/missing, stake is invalid, edge is non-positive, or risk flags make the decision unsuitable.
- Show the calculation chain: fair probability, market price, edge, EV, Kelly, correlation scale, bankroll cap, and final stake.

### 4. Portfolio, Paper Trading, and Risk

- Add a dedicated paper-trading portfolio surface using existing account, analytics, positions, settlement, and reset commands.
- Show cash, equity, open exposure, realized/unrealized PnL, win rate, drawdown, open positions, and recent lots.
- Gate destructive reset behind confirmation and a visible starting-balance field.
- Add export support for the paper journal.

### 5. Analyst Experience

- Share one market context packet between dashboard detail and analyst chat.
- Require actionable forecasts to include ticker, side, price, fair probability, edge, risks, and pass/enter criteria.
- Track provenance: model, timestamp, context freshness, data sources, and token usage.
- Add advanced model comparison with clear cost and latency warnings.

### 6. Data Quality and Performance

- Decouple cached dashboard reads from long full-catalog refreshes.
- Add a fetch-in-progress guard to avoid duplicate warmups.
- Persist compact market summaries for instant next-launch paint.
- Record Kalshi API rate-limit and fallback endpoint status.
- Avoid blocking UI interactions on snapshot writes.

### 7. Calibration and ML

- Implement volatility-adjusted Kelly from historical Brier/calibration results.
- Replace sports-prop ML assumptions with Kalshi-native category features.
- Build category classifiers only when enough resolved examples exist.
- Report Brier, ECE, CLV, ROI, pass rate, and confidence calibration by category.

### 8. Security and Desktop Hardening

- Move OpenRouter, Kalshi, Discord, Telegram, weather, and sports API secrets into the OS credential store.
- Leave only non-secret preferences in JSON config.
- Set a real Tauri CSP.
- Minimize Tauri permissions.
- Redact secrets from logs, errors, exports, placeholders, and diagnostics.

### 9. Notifications and Diagnostics

- Separate Kalshi market notifications from legacy sports notifications.
- Add a notification center for market resolution, paper settlement, stale watchlist, and grading completion.
- Add a diagnostics screen with app version, DB path, config path, cache age, API health, background task status, and recent redacted errors.
- Add a redacted support bundle export.

### 10. Quality Gates

- Follow test-driven development for behavior changes.
- Un-ignore or replace stale Rust tests.
- Add frontend component tests and a Tauri smoke path.
- Keep `npm run typecheck --prefix src-ui`, frontend tests, `cargo test`, healthcheck, and production build passing before release.

## Acceptance Criteria for Streams 1-3

- The first screen is Kalshi Markets, not a sports prop board.
- Primary navigation contains no Prop Board entry.
- Dashboard cards include category, status, close time, spread, volume, and liquidity.
- Dashboard shows cache/freshness state and partial/full catalog state.
- Market detail exposes mechanics, risk flags, and explicit paper decision actions.
- Non-positive edge disables record actions and routes the user to Watch/Pass.
- Existing typecheck and Rust tests still pass.

## Acceptance Criteria for Streams 4-6

- Paper Trades shows account equity, cash, open positions, return, win rate, and unrealized PnL.
- Paper Trades exposes open positions with ticker, side, quantity, average entry, mark, market value, and unrealized PnL.
- Paper settlement and reset are available from explicit controls, with reset protected by confirmation.
- Market detail can hand a selected market context packet into Analyst chat without requiring the user to retype ticker, price, edge, liquidity, spread, or risk flags.
- Analyst chat opens with the market prompt as an editable draft rather than auto-submitting it.
- Dashboard bootstrap includes backend-sourced market/category counts, generated timestamp, cache status, partial/full catalog state, and data-quality notes.
- Existing frontend tests, typecheck, production build, and Rust tests still pass.

## Acceptance Criteria for Streams 7-8

- Market detail shows the current calibration artifact, source, fit count, raw probability, calibrated probability, adjustment, sample status, and volatility haircut.
- Calibration status is sourced from the Rust calibration layer that already powers risk-adjusted paper decisions.
- Settings shows a security posture section with CSP status, redaction status, protected secret-field count, and vault-migration warning.
- Secret inputs remain blank with masked placeholders; saving preserves existing secrets unless a replacement is entered.
- UI-facing security posture and diagnostics never include raw API keys, passwords, bot tokens, or webhook URLs.
- Tauri config uses an explicit CSP instead of `null`.
- Existing frontend tests, typecheck, production build, and Rust tests still pass.
