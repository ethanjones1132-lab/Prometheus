# Kalshi Monster v0.8.0

**AI-powered prediction market intelligence engine** — A Tauri 2 + Rust desktop application that connects to OpenRouter API to deliver probability-weighted market assessments with real-time Kalshi data, liquidity analysis, and risk-adjusted decision modeling.

> **Analytics and research only.** This app never places, sizes, sends, or
> auto-submits a real order. It reads public market data and produces analysis.

## What It Is

Kalshi Monster is a desktop app designed to give AI models deep analytical capabilities for prediction markets. Every assessment provides:

- **Real-time Kalshi market data** for sports, politics, economics, and finance
- **Risk-adjusted expected value (EV) analysis** with Kelly criterion sizing
- **Downside control** and overconfidence shrinkage modeling
- **Market liquidity and bid-ask spread awareness**
- **Structured prediction tracking** and performance calibration
- **Multi-source market data** with automated failover
- **Intelligent risk-flagging** for extreme probabilities or market friction

## Edge Validation & Calibration (frozen 2026-06-11)

Kalshi Monster shares the evaluation stack built for the Monster apps. Its edge
math is the shared `../monster-edge-core` crate (it was byte-identical to
prizepicks-monster's), and it consumes the same `../edge-eval` engine.

**Market-calibration benchmark** — `eval-cli` fetches settled Kalshi markets
from the public API for liquid series (weather dailies, Fed, CPI, payrolls),
reconstructs each market's price path from candlesticks, and scores the
**entry price as a forecast** over **1,008** resolved markets:

| Metric | Value |
|---|---|
| Brier score | 0.129 |
| ECE (calibration error) | 0.037 |
| Brier skill score | **+0.28** |

The market's own price is well-calibrated — this is the **benchmark the app's
own forecasts must beat**, and it exercises closing-line value (CLV) end to end
(`closing_prob` = final candle). The market-prior calibrator is saved to
`reports/market-calibrator.json`.

**The same calibration as PrizePicks** — `analyze_single_prop` applies the
measured isotonic calibrator (override file
`~/.openclaw/kalshi-monster/calibrator.json`, else the embedded artifact). The
`predictions/tracker` Brier bug (graded `confidence_score` as a probability,
counted pushes as losses) was fixed to use the shared engine.

**Current measurement status** — the app's own LLM forecasts have **no graded
history yet** (the live DB holds only Pending rows). Once `predictions.db`
accumulates resolved Win/Loss rows, the same engine scores them via
`eval_adapter` — and unlike PrizePicks, Kalshi's public API makes a full year
of resolved-market history retrievable for benchmarking.

Re-run (in-app, via `eval_adapter`):
```bash
cd src-tauri && cargo test eval_adapter::
```
Offline benchmark artifacts live in `reports/` (`backtest-report.md`, `market-calibrator.json`).
The standalone `eval-cli` crate is not in this repo; calibration runs through the shared `edge-eval` library dependency.

## Architecture

```
kalshi-monster/
├── src-tauri/           # Rust backend
│   ├── src/
│   │   ├── lib.rs       # App entry, state management
│   │   ├── config.rs    # App config, model list, API status
│   │   ├── commands/    # Tauri command handlers
│   │   ├── chat/        # Chat sessions + OpenRouter API
│   │   ├── predictions/ # Prediction tracking + calibration
│   │   └── kalshi/      # Kalshi market data API integration
│   └── Cargo.toml
├── src-ui/              # React + TypeScript frontend
│   ├── src/
│   │   ├── App.tsx              # Main app shell
│   │   ├── components/
│   │   │   ├── KalshiView.tsx       # Live Kalshi market dashboard
│   │   │   ├── ChatView.tsx         # AI chat interface
│   │   │   ├── KalshiPredictionsPanel.tsx  # Paper trade log + analytics
│   │   │   └── ...
│   │   └── ...
│   └── package.json
└── README.md
```

## Getting Started

### Prerequisites
- [Rust](https://rustup.rs/) (1.85+)
- [Node.js](https://nodejs.org/) (18+)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS

### Development
1. Clone the repository.
2. Install dependencies: `npm install --prefix src-ui`
3. Run in dev mode: `npm run dev --prefix src-ui` (or `cargo tauri dev` from `src-tauri`)

```bash
# Install frontend dependencies
cd src-ui && npm install

# Run in development mode (from project root)
npm run tauri dev

# Build for production
npm run tauri build
```

### First Run
1. Launch the app
2. Go to **Settings** → Enter your [OpenRouter API key](https://openrouter.ai/keys)
3. Click **Test Connection** to verify
4. Select your preferred model (Claude Sonnet 4 recommended)
5. Click **"+ New Prediction"** in the sidebar
6. Start asking about player props!

## Example Queries

- "What are today's best player prop picks?"
- "Analyze Mahomes passing yards prop vs BUF"
- "Give me your top 3 rushing yard props this week"
- "Over/Under picks for TNF game"
- "Highest confidence picks across all games today"

## Response Format

The AI responds with structured predictions:

```
🏈 PICK: Over 285.5 for Patrick Mahomes — Passing Yards
📊 REASONING: Mahomes averages 280 yards/game this season...
⚡ CONFIDENCE: Medium
📈 PROBABILITY: 55% Over
⚠️ RISK: Wind gusts up to 18 mph could suppress passing
```

## Configuration

Config is stored at `~/.openclaw/kalshi-monster/config.json`:

```json
{
  "openrouter_api_key": "sk-or-v1-...",
  "openrouter_base_url": "https://openrouter.ai/api/v1",
  "selected_model": "anthropic/claude-sonnet-4-20250514",
  "system_prompt": "...",
  "max_context_players": 50
}
```

## Tech Stack

- **Desktop Framework:** Tauri 2 (Rust)
- **Frontend:** React 18 + TypeScript + Tailwind CSS v4
- **AI API:** OpenRouter (multi-model gateway)
- **Styling:** Custom "Gothic Maximalist" dark theme
- **State:** Tokio async runtime + Tauri managed state

## Roadmap

### Edge Validation milestone (2026-06-11) - Current
- [x] **Shared `edge-eval` + `monster-edge-core`** — same evaluation engine and edge math as prizepicks-monster (one crate, one calibrator, no divergent copies).
- [x] **Resolved-market backfill + CLV backtest** — 1,008 settled Kalshi markets scored; market entry price confirmed well-calibrated (Brier 0.129, BSS +0.28) as the benchmark.
- [x] **Measured calibration wired into the live path** — isotonic calibrator applied in `analyze_single_prop`; `tracker` Brier bug fixed (Brier over stored probability, pushes/voids excluded).
- [x] **Baseline hygiene** — git baseline established; pre-existing `&str + &str` compile error fixed; dead lib-test target resurrected (8 stale tests quarantined with reasons for follow-up).

### v0.8.0
- [x] **ML predictions injected into AI chat prompt** — Trained ML model predictions are now automatically injected as system context in both streaming and non-streaming chat, giving the AI model ML-backed win probabilities for pending props
- [x] **ML disagreement highlighting** — When ML model disagrees with the AI's original probability, the conflict is flagged in the prompt for sharper analysis
- [x] **Feature importance tracking** — ML model feature importance displayed in the ML Predictions UI tab

### v0.7.0
- [x] Historical prop line movement tracking — snapshot and track how prop lines change over time, with charts and filtering

### v0.6.0
- [x] Live ESPN API integration for real-time schedule data
- [x] Prediction extraction and tracking from AI responses
- [x] Performance dashboard with confidence-level breakdowns
- [x] 60+ player profiles with full splits (home/away, top-10/bottom-10 defense)
- [x] 32 team offense and defense rankings
- [x] Streaming chat responses (Server-Sent Events)
- [x] Player prop comparison tool with visual charts
- [x] Custom system prompts (risk tolerance, stat weighting, output format)
- [x] Export predictions to text/image/social media
- [x] Model comparison (same query to multiple models)
- [x] Parlay builder with correlation detection and EV calculation
- [x] Calibration metrics (Brier score, skill score, reliability diagram)
- [x] Multi-source data fallback (OpticOdds → Apify → Mock)
- [x] Live weather delta injection (Open-Meteo + OpenWeatherMap)
- [x] Sleeper API integration for injuries and player stats

### In Progress
- [x] SQLite-backed prediction storage (migrated from JSON files)
- [x] Bankroll management / Kelly criterion calculator
- [x] Multi-sport live data (NBA, MLB, NHL) — live scoreboard fetching implemented
- [x] NFL knowledge base fully populated — 18 QBs, 15 RBs, 20 WRs, 12 TEs, 32 defenses, 32 offenses with full stats
- [x] Multi-sport player databases — 25 NBA players, 20 MLB players, 20 NHL players with full stats
- [x] Dark/light theme toggle
- [x] Webhook notifications for game-day pick reminders
- [x] Player prop trend tracking with charts in TrendsView
- [x] Analysis engine — edge calculator, matchup analyzer, parlay correlation, prop scorer

### Future
- [x] Historical prop line movement tracking
- [ ] Machine learning model for prop prediction training

## License

MIT
