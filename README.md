<div align="center">

# 🏦 Kalshi Monster

**AI-powered command desk for Kalshi prediction markets** — browse markets, chat with an analyst, trade paper, and size positions with Kelly-calibrated edge math.

[![License](https://img.shields.io/badge/license-MIT-green)](#license)
[![Version](https://img.shields.io/badge/version-0.8.0-blue)]()
[![Language](https://img.shields.io/badge/language-Rust-000000)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20|%20macOS%20|%20Linux-lightgrey)]()

</div>

---

## Overview

Kalshi Monster is a **desktop command desk** for [Kalshi](https://kalshi.com) binary event contracts. It combines a live market browser, an AI chat analyst (powered by OpenRouter), a paper-trading engine with contract-side grading, and a staking assistant rooted in Kelly criterion math and Brier-score calibration.

Built with Rust + Tauri 2 for a native desktop experience and a React/Vite frontend, the app keeps all financial data local — your Kalshi credentials, prediction history, and bankroll config live on your machine.

```
Kalshi API → [Market Cache] → [Dashboard / Search]
Player Prop APIs → [Edge Calculator] → [Kelly Sizing]
AI Analyst (OpenRouter) → [Extract Predictions] → [Track & Grade]
Paper Trader → [Lot Journal] → [PnL / Equity Reports]
```

---

## Features

| Area | Capability |
|------|-----------|
| **Market Browser** | Browse, search, filter Kalshi markets; view orderbooks, price history, category stats |
| **Dashboard** | One-shot bootstrap with active markets, ML readiness hints, cache state |
| **AI Analyst Chat** | Chat via any OpenRouter model with streaming, Kalshi context injection, auto-prediction extraction |
| **Edge Analysis** | Multi-sport player prop research (NFL, NBA, MLB, NHL, NCAAF, NCAAB) with edge % and quality tiers |
| **Paper Trading** | Immutable lot journal, cash account, equity snapshots — Kalshi contract-side grading |
| **Bankroll Management** | Full/quarter/fractional Kelly, flat/percentage/volatility-adjusted staking, daily/weekly exposure caps |
| **ML Predictor** | scikit-learn GradientBoosting classifier trained on your prediction history + line movements |
| **Calibration** | Beta-binomial Brier-score calibration with runtime artifact overrides |
| **Notifications** | Game-start / final / auto-grade desktop alerts via OS notification center |
| **Bot Integration** | Schedule daily picks, game alerts, and grading results to Discord or Telegram |
| **Line Movement Tracking** | Snapshots line movements over time with per-league/stat-category detail |
| **Live Scores** | Multi-sport scoreboard (ESPN) and Sleeper fantasy data |
| **CSV Export** | Export prediction history for offline analysis |

---

## Quick Start

### Prerequisites

- **Rust** toolchain (edition 2021, rust-version 1.85+)
- **Node.js** 18+ and npm (for the React frontend)
- **Python** 3.10+ with scikit-learn (for ML predictions, optional)

### Build & Run

```bash
# Clone the repo
git clone https://github.com/jonesinsrc/kalshi-monster
cd kalshi-monster

# Frontend dependencies
cd kalshi-monster/src-ui
npm install

# Run in development mode
npm run dev              # (in one terminal — starts Vite dev server)
cd ../src-tauri
cargo tauri dev          # (in another — launches the desktop app)

# Or build for production
cd ../src-tauri
cargo tauri build
```

### First Launch

1. Go to **Settings** and configure your **OpenRouter API key** (required for chat).
2. Optionally set your **Kalshi email/password** for live market data and paper trading.
3. Start browsing markets on the **Command desk** tab, or open a chat with the **Analyst**.

---

## Screenshots

> ⚡ *Screenshots pending — add assets in `kalshi-monster/docs/images/` and reference with standard markdown image syntax.*

---

## Usage

### Market Browser ("Command desk")

Browse active Kalshi markets grouped by category. Click any market to see its orderbook, price history, and a detailed view. The dashboard loads a quick cache at startup so it's ready instantly.

### AI Analyst ("Analyst" chat)

Ask the analyst anything — market research, prop analysis, portfolio questions. The analyst has live access to Kalshi market data, sports player stats, and weather information. Predictions it makes are automatically extracted and tracked.

### Paper Trading ("Paper portfolio")

Record paper trades on Kalshi contracts. Positions are graded when the underlying market resolves. View PnL, equity curve, and position-level analytics.

### Edge Analysis

Enter a player, stat category, line, and projection. The engine computes edge %, win probability, expected value, and Kelly stake. Scores are tiered: **Elite**, **Strong**, **Playable**, **Marginal**, **Avoid**.

### ML Predictor

Train a GradientBoosting model on your historical predictions and line movements. Results are surfaced on the dashboard and injected into the analyst's context for sharper prompts.

---

## Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `openrouter_api_key` | string | — | OpenRouter API key for chat |
| `selected_model` | string | `nemotron-3-super-120b-a12b:free` | Default chat model |
| `risk_tolerance` | string | `moderate` | conservative / moderate / aggressive |
| `stat_weighting` | string | `balanced` | season_avg / last3 / matchup_adjusted / balanced |
| `kalshi_email` | string | — | Kalshi account email |
| `kalshi_poll_interval_secs` | u64 | 60 | Market poll interval |
| `kelly_fraction` | f64 | 0.25 | Kelly fraction for stake sizing |
| `max_bet_pct` | f64 | 0.05 | Max bet as fraction of bankroll |
| `theme` | string | `dark` | dark / light |

Config is stored at `~/.openclaw/kalshi-monster/config.json`.

---

## Architecture

### Project structure

```
kalshi-monster/              # Main Tauri desktop application
├── src-tauri/               # Rust backend — all business logic
│   └── src/
│       ├── lib.rs           # Tauri app setup, command registration, background tasks
│       ├── main.rs          # Entry point
│       ├── commands/        # Tauri command implementations (bridge to frontend)
│       ├── kalshi/          # Kalshi API client, market cache, grading, portfolio risk
│       ├── chat/            # OpenRouter integration, session management, prompt engineering
│       ├── predictions/     # Prediction storage (SQLite), tracking, grading
│       ├── analysis/        # Edge calculator, calibration resolver
│       ├── bot/             # Discord / Telegram bot integration
│       ├── notification.rs  # Game-day notification engine
│       ├── bankroll.rs      # Kelly criterion staking
│       ├── ml_predictor.rs  # Python-interop ML training & inference
│       ├── paper/           # Paper trading engine
│       ├── config.rs        # Application config
│       └── weather.rs       # OpenWeatherMap client
├── src-ui/                  # React + Vite + TypeScript frontend
│   └── src/
│       ├── App.tsx          # Tab navigation (markets / analyst / predictions / settings)
│       ├── components/      # KalshiView, ChatView, SettingsView, etc.
│       └── main.tsx         # Frontend entry point
monster-edge-core/           # Shared edge math library (also used by offline backtest CLI)
│   └── src/lib.rs           # Edge calculation, calibration adjustment
edge-eval/                   # Calibration / backtest / recalibration engine
│   └── src/
│       ├── lib.rs
│       ├── calibration.rs   # Beta-binomial calibrator
│       └── types.rs         # Calibrator, recalibrate
```

### Key design decisions

- **Kalshi-first data flow** — The app defaults to Kalshi market context for the analyst, not sports data. The first thing opened is the market dashboard.
- **Two-phase caching** — Dashboard loads a quick cache instantly at startup (sub-second), then warms the full catalog after 8 seconds for complete searches.
- **SQLite persistence** — All predictions, line movements, price snapshots, and market caches live in a local SQLite database. No cloud dependencies.
- **Runtime calibration** — Calibration artifacts are loaded from disk at `~/.openclaw/kalshi-monster/calibrator.json` with a fallback to the embedded build artifact, so re-fits can ship without a rebuild.
- **Auto-grade pipeline** — Background tasks poll Kalshi for resolved markets, auto-grade paper trades, and settle open lots.
- **Streaming chat** — OpenRouter streaming is forwarded to the frontend as Tauri events (`stream-chunk`, `stream-thought`, `stream-done`).

---

## Installation

### From source (all platforms)

```bash
# Requires: Rust 1.85+, Node.js 18+, npm
git clone https://github.com/jonesinsrc/kalshi-monster
cd kalshi-monster/kalshi-monster/src-ui && npm install
cd ../src-tauri && cargo tauri build
```

The bundled installer will be in `src-tauri/target/release/bundle/`.

### System requirements

| OS | Status |
|----|--------|
| Windows 10+ | ✅ Tested |
| macOS 13+ | ✅ Tested |
| Linux (X11/Wayland) | ⚠️ Community |

---

## Technical Highlights

### Edge calculation

The edge engine in `monster-edge-core` computes win probability from line + projection delta, then applies a beta-binomial Brier-score calibration that shrinks toward the prior when fitting data is thin. The pipeline:

```
Projection vs Line → Raw Win Prob → Calibrator → Calibrated Win Prob → Kelly Fraction → Stake
```

### Calibration safety

Calibration is conservative by construction — the beta-binomial method never amplifies edge on thin data. A runtime override file lets you ship updated calibrations without recompiling.

### ML integration

The ML predictor exports features from SQLite (prediction history + line movement snapshots), shells out to a Python scikit-learn GradientBoosting classifier, and stores predictions back in SQLite. Results surface on the dashboard and are injected into the analyst's system prompt.

---

## Contributing

1. Fork the repo
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit your changes (`git commit -am 'Add my feature'`)
4. Push to the branch (`git push origin feat/my-feature`)
5. Open a Pull Request

### Development guidelines

- Rust code: follow existing module structure; use `thiserror` for error types; add Tauri commands in `commands/mod.rs`
- Frontend: React functional components, TypeScript strict mode, Vitest for tests
- SQL migrations: additive only (no destructive schema changes)
- Calibration changes: ship a new `calibrator.json` artifact, don't break the JSON schema

---

## Changelog

| Date | Version | Change |
|------|---------|--------|
| 2026-06 | 0.8.0 | Dashboard ML CV, sidecar insight rail, Phase 3 ML readiness hints |
| 2026-05 | 0.7.0 | Kalshi-first mode, paper trading engine, auto-grade pipeline |
| 2026-04 | 0.6.0 | OpenRouter streaming, notification engine, bot integration |
| 2026-03 | 0.5.0 | Edge calculator with beta-binomial calibration |
| 2026-02 | 0.4.0 | Multi-sport scoreboard, line movement tracking |
| 2026-01 | 0.3.0 | Kalshi market browser, orderbook, price history |
| 2025-12 | 0.2.0 | Initial chat analyst with prediction extraction |
| 2025-11 | 0.1.0 | MVP — OpenRouter chat + PrizePicks prop research |

---

## FAQ

**Q: Do I need a Kalshi account to use this?**
A: No. You can use the AI analyst and prop research without one. Paper trading and live market data require Kalshi credentials.

**Q: Is this giving me financial advice?**
A: No. Kalshi Monster is a research tool. All staking calculations are informational. Trade at your own risk.

**Q: Where is my data stored?**
A: Entirely locally. Config, predictions, line movements, and market caches live in `~/.openclaw/kalshi-monster/` (SQLite DB + JSON files).

**Q: Can I use this without an OpenRouter API key?**
A: The market browser works without one. The AI analyst requires an OpenRouter key (free models available).

---

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

**Kalshi Monster** — Built with ❤️ for prediction market traders

[Issues](https://github.com/jonesinsrc/kalshi-monster/issues) · [Kalshi](https://kalshi.com) · [OpenRouter](https://openrouter.ai)

</div>