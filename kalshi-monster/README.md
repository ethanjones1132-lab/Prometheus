<div align="center">

# Prometheus · Kalshi Monster

**AI-powered prediction market intelligence — on your desktop.**  
Real-time markets · Risk-calibrated analysis · Paper trading · ML-boosted forecasts

[![License](https://img.shields.io/badge/license-MIT-green)](src-tauri/Cargo.toml)
[![Version](https://img.shields.io/badge/version-0.8.0-blue)](src-tauri/Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-222%20lib%20|%2040%20vitest-success)](scripts/agent-healthcheck.ps1)
[![Calibration](https://img.shields.io/badge/calibration-Brier%200.129%20|%20BSS%20+0.28-success)](reports/backtest-report.md)
[![Platform](https://img.shields.io/badge/platform-Windows%20|%20Linux%20|%20macOS-lightgrey)]()

</div>

---

## What is this?

**Kalshi Monster** is a desktop app that helps you analyze [Kalshi](https://kalshi.com/) prediction markets — the regulated CFTC exchange where you can trade on event contracts like "Will the Fed cut rates in September?" or "Will Team X win the Super Bowl?"

Think of it as your personal market research assistant. It pulls live market data from Kalshi's public API, runs it through an **edge-calibrated AI engine** (your choice of model via OpenRouter), and gives you structured, risk-aware assessments with Kelly criterion sizing, overconfidence shrinkage, and liquidity-awareness baked in.

> **This is an analytics tool, not a trading bot.** It never places, sizes, sends, or auto-submits a real order. It reads public market data and produces analysis — you make the final call.

> **Prometheus** is this GitHub repository. **Kalshi Monster** is the app.

---

## Who is this for?

- **Prediction market enthusiasts** who want deeper analysis than Kalshi's own interface provides
- **Traders** who want AI-assisted market scanning with built-in risk calibration, a paper trading engine, and a track record of calibration accuracy
- **Anyone curious about prediction markets** who wants to understand what the numbers actually mean — and whether they're any good at forecasting

---

## What can it do?

| Feature | How it works |
|:--------|-------------|
| **Live market dashboard** | Browse Kalshi markets in real time — sports, politics, economics, finance, weather. See bid/ask prices, volume, open interest, and 24-hour change at a glance. Filter by category, status (open/settled), and liquidity tiers. Click any market for a deep-dive detail panel. |
| **AI analysis with edge math** | Ask questions in plain English: "What's the best value in political markets right now?" or "Analyze the Fed rate decision market." The AI fetches live Kalshi data and runs it through the **edge engine** — which calculates expected value (EV), optimal Kelly fraction, overconfidence shrinkage (a configurable lambda that penalizes extreme probabilities), and liquidity-adjusted confidence. |
| **Risk-calibrated predictions** | Every analysis includes a structured forecast with an implied probability, edge calculation, risk assessment, and confidence level — all adjusted by the same isotonic calibrator used in the backtest that scored a **Brier of 0.129** and **Brier Skill Score of +0.28** over 1,008 resolved markets. This calibrator corrects for systematic over/under-confidence so your estimates are well-calibrated on average. |
| **Paper trading** | Track hypothetical trades without risking real money. The **paper trading engine** records each trade (market, direction, stake, filled price), accounts for Kalshi's fee structure (`order_fee` + `fee_multiplier`), and grades the trade when the market settles — showing you your running P&L, win rate, and calibration performance. You track a "cash" equity balance separate from open position values, and can view full position-level detail with realized + unrealized P&L. |
| **ML prediction booster** | A trained machine learning model scores every pending market and injects its probabilities into the AI's prompt before analysis. When the ML's estimate disagrees with the AI's, the conflict is flagged right in the analysis — giving you a "second opinion" on every call. The ML model's feature importance (which factors drove its prediction) is shown in an ML Predictions UI tab. |
| **Calibration tracking** | A dedicated Calibration dashboard shows how your predictions are performing over time — Brier score, calibration curves, and reliability diagrams. You can see whether you're overconfident or underconfident across different confidence buckets, and the same edge-engine calibration pipeline from the market backtest is applied to your live predictions once they resolve. |
| **Edge engine configuration** | Full control over the edge model: set the shrinkage lambda (how aggressively to pull extreme probabilities toward 0.5), configure Kelly multiplier, and view the active shrinkage curve in the Settings panel. The lambda is persisted across sessions and applied to every analysis. |
| **Line movement tracking** | Historical price charts for any market — snapshot how the market price moves over time with configurable timeframes. Spot trends, momentum shifts, and sentiment changes before the crowd. |
| **Multi-model comparison** | Send the same market analysis question to multiple AI models simultaneously and compare their answers side by side. |
| **Multi-source data failover** | Market data comes from Kalshi's live API, but the system supports automatic failover tiers: primary API → secondary API → cached local data → graceful degradation, so you're never left with a blank screen. |
| **Live weather data** | Weather conditions are injected into analyses for weather-dependent markets (temperature derivatives, event cancellation risk) via Open-Meteo + OpenWeatherMap APIs. |
| **Sleeper API integration** | Player injuries and status data for sports markets, fetched from the Sleeper API. |
| **Dark & light themes** | Two visual themes: a "Gothic Maximalist" dark theme with custom CSS, and a clean light mode. |
| **Webhook notifications** | Configurable webhooks for market settlement alerts, price level triggers, and daily pick reminders. |

---

## Edge calibration — the numbers

Kalshi Monster shares its evaluation stack with the other Monster apps through the shared `monster-edge-core` and `edge-eval` crates. This is not a toy — it's a measured, verified pipeline.

| Metric | What it measures | Score |
|--------|-----------------|-------|
| **Brier score** | How close predictions are to actual outcomes (0 = perfect, 1 = always wrong) | **0.129** |
| **ECE (Expected Calibration Error)** | How much predictions deviate from perfect calibration — the average gap between predicted probabilities and actual frequencies | **0.037** |
| **Brier Skill Score** | How much better the market price is than a naive 50/50 baseline (negative = worse than guessing, positive = useful signal) | **+0.28** |

The benchmark was run across **1,008 resolved Kalshi markets** — weather dailies, Fed rate decisions, CPI releases, payroll reports — using the market's entry price as a forecast. The market's own price turns out to be well-calibrated, and this is the benchmark any derived forecast must beat. The isotonic calibrator from that benchmark is applied live to every analysis in the app (loaded from `~/.openclaw/kalshi-monster/calibrator.json` or the embedded artifact).

Re-run the calibration:

```bash
cd src-tauri && cargo test eval_adapter::
```

Full results in [`reports/backtest-report.md`](reports/backtest-report.md).

---

## Architecture — the full picture

```
┌──────────────────────────────────────────────────────────────────────┐
│  React UI (src-ui/) — Vite + TypeScript + Tailwind CSS v4            │
│  KalshiView · ChatView · PredictionsPanel · CalibrationView · Props  │
│  Settings · WorldMarketsView · LineMovement charts · ML Predictions   │
│  PriceHistoryChart · ReliabilityDiagram                               │
│  Tauri IPC ↔ Rust backend commands                                    │
└────────────────────────────┬─────────────────────────────────────────┘
                             │
┌────────────────────────────▼─────────────────────────────────────────┐
│  Tauri / Rust (src-tauri/) — Tokio async runtime                      │
│  ┌────────────┐  ┌───────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ Kalshi API │  │ OpenRouter│  │ Predictions  │  │ Paper engine │  │
│  │ client     │  │ chat API  │  │ + calibration│  │ + Kelly calc │  │
│  └─────┬──────┘  └─────┬─────┘  └──────┬───────┘  └──────┬───────┘  │
│        │               │               │                  │          │
│        ▼               ▼               ▼                  ▼          │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  SQLite — predictions, sessions, paper trades, edge config  │    │
│  │  modules: predictions::db, paper::db, config::edge_config   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Fincept sidecar (bundled process via tauri-plugin-shell)   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Secrets keyring (OS credential store)                       │    │
│  └─────────────────────────────────────────────────────────────┘    │
└────────────────────────────┬─────────────────────────────────────────┘
                             │
                    ┌────────┴────────┐
                    ▼                 ▼
            Kalshi API          OpenRouter
         (public markets)      (AI models)

```

### How a market analysis flows

1. You type a question ("Analyze the Fed rate decision market") or click a market in the dashboard
2. The **Rust engine** fetches live data from the **Kalshi API** — current price, volume, bid/ask spread, order book depth, 24-hour change
3. The **edge engine** calculates:
   - **Expected value** — expected return vs the current price, adjusted for fees
   - **Kelly fraction** — the optimal fraction of your paper bankroll to allocate, given the edge and the odds
   - **Shrinkage-adjusted probability** — pushes extreme probabilities back toward 50% based on the configured lambda (default fitted from 1,008-market backtest)
   - **Liquidity score** — factors in bid-ask spread and depth to flag thin markets
4. The **ML prediction booster** checks its model for a probability estimate on this market — if found, it's injected as system context for the AI along with a "ML disagrees" flag if the two probabilities diverge
5. Everything is packaged into a prompt and sent to your chosen **OpenRouter model** (Claude, GPT, Gemini, DeepSeek — your pick)
6. The AI returns a structured analysis with a forecast, reasoning, risk assessment, and confidence
7. Results stream back to the UI in real time (SSE) — no waiting for the full response
8. The chat session, predictions, and any paper trades are saved to **SQLite**
9. When the market settles, the system grades the prediction via `resolve_forecasts_for_market`, logs the result to the calibration tracker, and updates your paper P&L

### Rust module breakdown

| Module | What it does |
|--------|-------------|
| `kalshi/` | Kalshi API client — market data, order book, trading endpoints, WebSocket streaming, market search, fast `/markets` cache path |
| `chat/` | OpenRouter integration — streaming and non-streaming chat, model selection, context management, ML injection |
| `commands/` | 10+ focused Tauri IPC command modules (split from a single 3400-line file) — session management, predictions, paper trading, edge config, fincept bridge, secrets |
| `predictions/` | Prediction tracking, calibration scoring (`eval_adapter`), isotonic calibrator, tracker Brier bug fix |
| `paper/` | Paper trade engine — `paper_breaker` module for stake scale, `paper_get_analytics` / `paper_get_positions` / `paper_settle_pending` / `paper_reset_account`, fee-aware grading |
| `fincept/` | Fincept sidecar bridge — manages the bundled analysis process |
| `config.rs` | App configuration — model lists, API status, edge config persistence |

### UI component breakdown

| Component | What it does |
|-----------|-------------|
| `KalshiView` | Main market dashboard — filterable, sortable market table with live price data |
| `ChatView` | AI chat interface with streaming responses, session management, inline rename |
| `KalshiPredictionsPanel` | Paper trade log + prediction analytics — P&L, win rate, calibration |
| `CalibrationView` | Brier score tracking, reliability diagrams, calibration curves, shrinkage λ display |
| `MarketDetailPanel` | Deep-dive on a single market — full order book, price history, line movement |
| `PriceHistoryChart` | Historical price chart for line movement tracking |
| `PropsView` | Player prop browsing for sports markets |
| `WorldMarketsView` | Cross-category market explorer |
| `SettingsView` | API key management (keyring), model selection, edge config (shrinkage λ), fincept sidecar control, system prompt |
| `ReliabilityDiagram` | Visual calibration chart — predicted probability vs actual outcome frequency |

---

## Milestones shipped

| What | What it means | When |
|------|--------------|------|
| **ML predictions in chat** | ML model probabilities are injected into every AI analysis automatically; ML-AI disagreement flagged | v0.8.0 |
| **ML feature importance** | Feature importance displayed in ML Predictions UI tab | v0.8.0 |
| **Fee-aware paper grading** | `contract_pnl` / `evaluate_bet` use persisted `fee_multiplier`; paper `place_trade` charges `order_fee` on open | 2026-07-12 |
| **Secrets keyring** | API keys migrated from plaintext config to OS credential store (Windows Credential Manager / macOS Keychain / Linux libsecret) | 2026-07-12 |
| **Edge config persistence** | `edge_config` table + IPC (`kalshi_get_edge_config`, `kalshi_set_edge_config`, `kalshi_set_shrinkage_lambda`) for full edge model control | 2026-07-11 |
| **Paper breaker stake multiplier** | `paper_breaker` module adjusts stake scaling based on circuit breaker state | 2026-07-11 |
| **Session rename** | Double-click inline rename of chat sessions (KB-2b) | 2026-07-13 |
| **10-module commands split** | 3400-line `commands/mod.rs` refactored into 10 focused modules | 2026-07-13 |
| **Fincept sidecar** | Settings panel management of bundled Fincept analysis process; `tauri.conf.release.json` with `externalBin` | 2026-07-13 |
| **Sync login gate** | `sync_kalshi_client_from_app_config` on dashboard bootstrap; empty-tape hints distinguish login vs public-catalog paths | 2026-07-13 |
| **Analyst settlement gates** | `market_gate` — markets require analyst confirmation before auto-settling | 2026-07-10 |
| **Web evidence injection** | `web_context` — web search results injected into analyses as supporting evidence | 2026-07-10 |
| **Line movement tracking** | Historical price chart snapshots with filtering | v0.7.0 |
| **Live ESPN data** | Real-time schedule data for sports markets | v0.6.0 |
| **Multi-source failover** | Automatic data-source failover (OpticOdds → Apify → Mock) | v0.6.0 |
| **Weather injection** | Live weather deltas from Open-Meteo + OpenWeatherMap | v0.6.0 |
| **Parlay builder** | Correlation detection + EV calculation | v0.6.0 |
| **Sleeper API** | Player injuries and stats | v0.6.0 |

Full changelog with test counts and commit history: [`PRIORITIES.md`](PRIORITIES.md).

---

## Quick start (for developers)

```bash
git clone https://github.com/ethanjones1132-lab/Prometheus.git
cd Prometheus

# Install frontend dependencies
cd src-ui && npm install && cd ..

# Run in dev mode (opens Tauri window)
cd src-tauri && cargo tauri dev
```

Dev server is at **http://localhost:1420** (Vite) — the Tauri window opens automatically.

### Prerequisites

| Tool | Required for | Minimum version |
|------|-------------|-----------------|
| [Rust](https://rustup.rs/) | Building the native engine | 1.85+ |
| [Node.js](https://nodejs.org/) | Frontend | 18+ |
| [OpenRouter API key](https://openrouter.ai/keys) | AI analysis (enter in app Settings) | — |

---

## First run

1. **Launch the app** — the market dashboard loads with public Kalshi data immediately (no API key needed to browse)
2. Go to **Settings** → Enter your [OpenRouter API key](https://openrouter.ai/keys)
3. Click **Test Connection** — the app verifies the key and shows available models
4. Pick your preferred model (Claude, GPT, Gemini, DeepSeek — whatever you like)
5. Browse live markets or open a chat and ask a question

### Example questions

- "What are the best value markets in politics right now?"
- "Analyze the Fed rate decision for September — what's the implied probability, what's the edge at the current price?"
- "Show me markets where the ML model disagrees with the crowd price"
- "What's a risk-weighted portfolio across my top 5 markets?"
- "How did my predictions perform this week?"
- "Compare the CPI release market across GPT-4o and Claude"

---

## Build & release

### Standard build

```bash
cd src-tauri && cargo tauri build
# → Platform installer: target/release/bundle/
```

### Release build with Fincept sidecar

```bash
python scripts/build_fincept_sidecar.py                     # Build the sidecar binary
cargo tauri build --config tauri.conf.release.json            # Bundle with externalBin
```

### Build + deploy dry-run

```bash
python scripts/build_fincept_sidecar.py --dry-run             # Verify packaging layout
cargo tauri build --config tauri.conf.release.json            # Full packaged release
```

---

## Configuration

### API keys (secure storage)

API keys are stored in your **OS credential store** (Windows Credential Manager / macOS Keychain / Linux libsecret), not in plaintext config files. The Settings UI handles the migration automatically.

### Config file

Located at `~/.openclaw/kalshi-monster/config.json`:

| Setting | What it controls |
|---------|-----------------|
| `openrouter_api_key` | OpenRouter API key (migrated to keyring on first save — the config only holds an empty placeholder after migration) |
| `openrouter_base_url` | API base URL (default: `https://openrouter.ai/api/v1`) |
| `selected_model` | Which AI model is active for analysis |
| `system_prompt` | Custom instructions telling the AI how to analyze markets — you can tune risk tolerance, stat weighting, output format |
| `shrinkage_lambda` | Overconfidence penalty — a higher value pulls extreme probabilities more aggressively toward 50%. Default is fitted from the 1,008-market backtest. |
| `max_context_players` | Maximum markets to include in a single analysis pass |

### Edge engine config (persisted in SQLite)

| Field | What it controls |
|-------|-----------------|
| `shrinkage_lambda` | Configured in Settings → Edge engine — NaN means "use fitted default" |
| `kelly_multiplier` | Kelly fraction scale (0.0–1.0) — conservatism factor on the optimal bet size |
| `fee_multiplier` | Custom Kalshi fee rate for P&L calculations |
| `stake_multiplier` | Paper trading stake scaling |

### Environment

- **Tauri 2** with plugins: `shell` (sidecar), `opener` (browser links), `notification` (webhooks), `log` (tracing)
- **CSP** allows connections to: Kalshi API, OpenRouter API, OpenCode API, and localhost IPC
- **Token timeout** configured per-model in the inference feedback system

---

## Verification

### Full health check (from repo root)

```powershell
.\scripts\agent-healthcheck.ps1
```

Checks: Rust compile, UI typecheck, Kalshi-specific tests, paper module wiring, roadmap presence.

### Individual checks

```bash
cargo check                                        # Rust lint
cd src-ui && npx tsc --noEmit                      # UI typecheck
cd src-ui && npx vitest run                        # UI tests (40+)
cargo test kalshi::                                 # Kalshi client tests
cargo test paper_breaker                            # Paper trading tests
cargo test eval_adapter::                           # Calibration pipeline tests
```

---

## Repository layout

```
Prometheus/
├── src-tauri/               # Rust — engine, API clients, AI chat, predictions, calibration
│   ├── src/
│   │   ├── commands/        #   ~10 IPC modules: paper, predictions, config, fincept, secrets...
│   │   ├── chat/            #   OpenRouter streaming + non-streaming, ML injection
│   │   ├── predictions/     #   Tracking, calibration (eval_adapter), isotonic calibrator
│   │   ├── kalshi/          #   API client, market data, order book, cache
│   │   ├── paper/           #   Paper trade engine, breaker, Kelly calc
│   │   ├── fincept/         #   Sidecar bridge, process management
│   │   ├── config.rs        #   App configuration + edge config
│   │   └── secrets.rs       #   OS credential store abstraction
│   ├── binaries/            #   Sidecar binary stubs (fincept-sidecar)
│   └── Cargo.toml           #   crate: kalshi-monster v0.8.0
├── src-ui/                  # React + TypeScript + Tailwind CSS v4
│   ├── src/
│   │   ├── components/      #   KalshiView, ChatView, PredictionsPanel, CalibrationView, ...
│   │   └── App.tsx          #   Main shell with routing
│   └── package.json
├── docs/                    # Plans and architecture decisions
├── scripts/                 # Build, deploy, and healthcheck scripts
├── reports/                 # Calibration artifacts, backtest reports
├── release/                 # Release artifacts
├── AGENTS.md                # Rules for autonomous coding agents
├── PRIORITIES.md            # Full changelog (594 lines) and improvement backlog
└── ROADMAP.md               # Phased development plan

```

---

## Tech stack (the details)

| Layer | Technology | Version / Specifics |
|-------|-----------|-------------------|
| **Desktop framework** | Tauri 2 | v2.11.1, custom protocol, background thread bootstrap |
| **Backend** | Rust | 2021 edition, rust-version 1.85, Tokio async runtime |
| **HTTP client** | reqwest | v0.12, JSON + blocking + streaming features |
| **Serialization** | serde + serde_json | Full derive support |
| **UUID** | uuid | v1, v4 feature |
| **Frontend** | React 18 + TypeScript | Vite, Tailwind CSS v4 |
| **AI gateway** | OpenRouter | Multi-model proxy, SSE streaming |
| **Edge math** | shared `monster-edge-core` + `edge-eval` | Same crate as PrizePicks Monster; isotonic calibration, Kelly sizing, shrinkage |
| **Database** | SQLite | rusqlite (via tauri), 3 tables: predictions, edge_config, session store |
| **Credential storage** | OS keyring | Windows Credential Manager, macOS Keychain, Linux libsecret |
| **Sidecar** | Fincept | Bundled analysis process via tauri-plugin-shell |
| **Styling** | Custom "Gothic Maximalist" dark theme | CSS variables for dark/light mode toggle |

---

## Styling

The app ships with a **Gothic Maximalist** dark theme — deep backgrounds, high-contrast accent colors, and a moody aesthetic designed for serious market analysis sessions. A light mode toggle is available in Settings.

The theme is built with CSS custom properties — every color is a variable, so custom theming is trivially achievable by overriding the root variables.

---

## License

MIT — see [`Cargo.toml`](src-tauri/Cargo.toml).

---

<div align="center">

**Prometheus / Kalshi Monster** — Built with Rust + TypeScript

[GitHub](https://github.com/ethanjones1132-lab/Prometheus) · [Issues](https://github.com/ethanjones1132-lab/Prometheus/issues)

</div>
