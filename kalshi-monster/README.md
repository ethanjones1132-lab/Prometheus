<div align="center">

# Prometheus · Kalshi Monster

**AI-powered prediction market intelligence — on your desktop.**  
Real-time markets · Smart analysis · Your money, your rules

[![License](https://img.shields.io/badge/license-MIT-green)](src-tauri/Cargo.toml)
[![Version](https://img.shields.io/badge/version-0.8.0-blue)](src-tauri/Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-222%20lib%20|%2040%20vitest-success)](scripts/agent-healthcheck.ps1)
[![Platform](https://img.shields.io/badge/platform-Windows%20|%20Linux%20|%20macOS-lightgrey)]()

</div>

---

## What is this?

**Kalshi Monster** is a desktop app that helps you analyze prediction markets on [Kalshi](https://kalshi.com/) — the regulated exchange where you can trade on events like "Will the Fed cut rates in September?" or "Will Team X win the Super Bowl?"

Think of it as your personal market research assistant. It pulls live market data from Kalshi, runs it through AI analysis (via OpenRouter — your choice of model), and gives you structured, risk-aware assessments. It never places real trades — it's an analytics tool, not a trading bot.

> **Prometheus** is this GitHub repository. **Kalshi Monster** is the app. Same project, two names.

---

## Who is this for?

- **Prediction market enthusiasts** who want deeper analysis than Kalshi's own interface provides
- **Traders** who want AI-assisted market scanning with risk calibration and Kelly sizing built-in
- **Anyone curious about prediction markets** who wants to understand what the numbers actually mean

---

## What can it do?

| What you see | What it means for you |
|:---|---|
| **Live market dashboard** | Browse Kalshi markets in real time — sports, politics, economics, finance, weather. See prices, volumes, and bid-ask spreads at a glance. |
| **AI-powered analysis** | Ask questions in plain English: "What's the best value in political markets right now?" or "Analyze the Fed rate decision market." The AI reads live data and gives you a structured answer. |
| **Smart risk calibration** | Every analysis includes expected value (EV), Kelly criterion sizing, and overconfidence shrinkage — so you don't bet more than the math supports. |
| **Paper trading** | Track hypothetical trades to test your strategies. The system grades them when markets settle and shows you your running P&L, win rate, and calibration score. |
| **ML prediction boost** | A machine learning model scores each pending market and injects its probabilities into the AI's prompt — you see when the ML disagrees with the AI's estimate, giving you a second opinion. |
| **Calibration tracking** | See how well your predictions are performing over time — Brier score, reliability diagrams, calibration curves. Know if you're overconfident or underconfident. |
| **Multi-model chat** | Compare the same question across different AI models. See which model gives you better analysis for different market types. |
| **Line movement tracking** | Watch how market prices change over time with historical charts and filtering — spot trends before the crowd. |
| **Your choice of AI** | Use any model available on OpenRouter — Claude, GPT, Gemini, DeepSeek. Switch any time. |
| **Dark & light themes** | A "Gothic Maximalist" dark theme (or light mode if that's your style). |

---

## Quick look under the hood (the 30-second version)

```
You ask a question about a market
        │
        ▼
  React UI ─────► Rust engine (Kalshi + AI)
                        │
                   ┌────┴────┐
                   ▼         ▼
             Kalshi API   OpenRouter
            (live data)   (AI models)
```

- **React UI** — The dashboard you see and interact with (chat, markets, predictions, calibration).
- **Rust/Tauri engine** — The brain. Fetches live market data, runs AI analysis, manages your paper portfolio, tracks calibration, and stores everything in SQLite.
- **Kalshi API** — Real-time market prices, order books, settled events, and trading history (read-only — no trades placed).
- **OpenRouter** — Your choice of AI models for market analysis (Claude, GPT, Gemini, etc.).

---

## What's been happening lately

| Recent milestone | What it means |
|:---|---|
| **ML predictions in every chat** | The ML model's win probabilities are automatically injected into every AI analysis — you see its disagreement flag when it thinks differently from the LLM. |
| **Fee-aware paper trades** | Paper trades now account for Kalshi's actual fee structure, so your simulated P&L is realistic. |
| **Credentials stored securely** | Your API keys are now stored in your OS credential store (keyring), not in a plaintext config file. |
| **Seamless market sync** | Credentials sync automatically between app config and the Kalshi client — log in once, the rest just works. |
| **Edge calibration controls** | Manually override the shrinkage lambda to dial in your edge model's aggressiveness. |
| **Session management** | Rename your chat sessions with an inline double-click — keep your analyses organized. |
| **Fincept sidecar (phase 1)** | Settings panel manages the Fincept analysis engine as a bundled sidecar process. |
| **Analyst settlement gates** | Markets can't auto-settle without analyst confirmation — prevents premature grading. |

Full changelog with test counts and commit history: [`PRIORITIES.md`](PRIORITIES.md).

---

## Quick start (for developers)

```bash
git clone https://github.com/ethanjones1132-lab/Prometheus.git
cd Prometheus

# Install frontend dependencies
cd src-ui && npm install && cd ..

# Run in development mode
cd src-tauri && cargo tauri dev
```

The dev server is at **http://localhost:1420** (Vite) — the Tauri window opens automatically.

### What you need

| Tool | For | Version |
|------|-----|---------|
| [Rust](https://rustup.rs/) | Building the native desktop engine | 1.85+ |
| [Node.js](https://nodejs.org/) | Frontend | 18+ |
| [OpenRouter API key](https://openrouter.ai/keys) | AI analysis (enter in app Settings) | — |

---

## First run

1. Launch the app
2. Go to **Settings** → Enter your [OpenRouter API key](https://openrouter.ai/keys)
3. Click **Test Connection** to verify
4. Pick your preferred model
5. Browse live markets or start a chat and ask a question

### Example questions

- "What are the best value markets in politics right now?"
- "Analyze the Fed rate decision for September"
- "What's the implied probability of this market, and is there edge?"
- "Give me a risk-adjusted portfolio across my top 5 markets"
- "Show me markets where the ML disagrees with the crowd price"

---

## Build & release

```bash
cd src-tauri && cargo tauri build
# → Platform installer in target/release/bundle/
```

For release packaging with the Fincept sidecar:

```bash
python scripts/build_fincept_sidecar.py          # Build the sidecar binary
cargo tauri build --config tauri.conf.release.json  # Bundle with externalBin
```

---

## Configuration

Settings are stored in two places:

| What | Where | Why |
|------|-------|-----|
| **API keys** | OS keyring (Windows Credential Manager / macOS Keychain / Linux libsecret) | Secure — no plaintext on disk |
| **App settings** | `~/.openclaw/kalshi-monster/config.json` | Model selection, system prompt, risk config |

| Setting | What it controls |
|---------|-----------------|
| `openrouter_api_key` | API key (migrated to keyring on save) |
| `selected_model` | Which AI model to use for analysis |
| `system_prompt` | Custom instructions for how the AI analyzes markets |
| `shrinkage_lambda` | Overconfidence penalty strength — higher = more conservative |
| `max_context_players` | Max markets to analyze in one pass |

---

## Verify it works

```bash
cargo check                                        # Rust lint
cd src-ui && npx tsc --noEmit && npx vitest run    # UI typecheck + tests
cargo test kalshi::                                 # Kalshi-specific tests
```

Or the full health check (from repo root):

```powershell
.\scripts\agent-healthcheck.ps1
```

Checks: Rust compile, UI typecheck, Kalshi tests, paper module presence, roadmap files.

---

## How it's built

| Layer | What's inside |
|-------|--------------|
| **Rust engine** | Tauri 2, Tokio async, Kalshi API client, OpenRouter chat, SQLite persistence, edge engine (calibration, Kelly sizing, shrinkage) |
| **React UI** | TypeScript, Tailwind CSS v4, Vite, custom Gothic Maximalist dark theme |
| **AI** | OpenRouter gateway (Claude, GPT, Gemini, DeepSeek — your pick) with streaming responses |
| **ML** | Injected prediction booster that flags disagreements with AI estimates |
| **Persistence** | SQLite — sessions, predictions, edge config, calibration history, paper trades |
| **Sidecar** | Fincept analysis engine as a bundled process for additional compute |

---

## Repository layout

```
Prometheus/
├── src-tauri/               # Rust — engine, API clients, AI chat, predictions, calibration
│   ├── src/
│   │   ├── commands/        # Tauri command handlers
│   │   ├── chat/            # Chat sessions + OpenRouter integration
│   │   ├── predictions/     # Prediction tracking + calibration
│   │   ├── kalshi/          # Kalshi market data API
│   │   ├── paper/           # Paper trading engine
│   │   ├── fincept/         # Fincept sidecar bridge
│   │   └── config.rs        # App config management
│   ├── binaries/            # Sidecar binary stubs
│   └── Cargo.toml
├── src-ui/                  # React TypeScript frontend
│   ├── src/
│   │   ├── components/      # Dashboard, chat, markets, predictions, calibration views
│   │   └── App.tsx          # Main app shell
│   └── package.json
├── docs/                    # Plans and architecture decisions
├── scripts/                 # Build and healthcheck scripts
├── reports/                 # Calibration artifacts and evaluation reports
├── AGENTS.md                # Rules for autonomous coding agents
├── PRIORITIES.md            # Full changelog and improvement backlog
└── ROADMAP.md               # Phased development plan
```

---

## Technical architecture (the full picture)

```
┌──────────────────────────────────────────────────────────────────────┐
│  React UI (src-ui/) — Vite + TypeScript + Tailwind                    │
│  Dashboard · Chat · Markets · Predictions · Calibration · Settings     │
│  Tauri IPC ↔ Rust commands                                            │
└────────────────────────────┬─────────────────────────────────────────┘
                             │
┌────────────────────────────▼─────────────────────────────────────────┐
│  Tauri / Rust (src-tauri/)                                             │
│  ┌─────────┐  ┌──────────┐  ┌────────────┐  ┌──────────────────┐     │
│  │ Kalshi  │  │ Chat     │  │Predictions │  │ Paper trading    │     │
│  │ Client  │  │ (OpenRtr)│  │+ Calib.    │  │ + Kelly sizing   │     │
│  └────┬────┘  └────┬─────┘  └──────┬─────┘  └────────┬─────────┘     │
│       │            │              │                  │               │
│       ▼            ▼              ▼                  ▼               │
│  ┌─────────────────────────────────────────────────────────┐        │
│  │  SQLite — sessions · predictions · edge config · paper  │        │
│  └─────────────────────────────────────────────────────────┘        │
│  ┌─────────────────────────────────────────────────────────┐        │
│  │  Fincept sidecar (bundled process)                      │        │
│  └─────────────────────────────────────────────────────────┘        │
└────────────────────────────┬─────────────────────────────────────────┘
                             │
                    ┌────────┴────────┐
                    ▼                 ▼
            Kalshi API          OpenRouter
         (public markets)      (AI models)
```

**How a market analysis flows:**
1. You type a question or browse a market in the dashboard
2. The Rust engine fetches live data from Kalshi (price, volume, order book, settled history)
3. The edge engine calculates expected value, Kelly sizing, and shrinkage-adjusted probabilities
4. The ML prediction booster checks if its model has a probability estimate for this market
5. Everything is packaged together and sent to the selected AI model via OpenRouter
6. The AI returns a structured analysis — displayed in real time in the chat panel
7. If you paper-trade based on the analysis, it's logged to SQLite and graded when the market settles

---

## License

MIT — see [`Cargo.toml`](src-tauri/Cargo.toml).

---

<div align="center">

**Prometheus / Kalshi Monster** — Built with Rust + TypeScript

[GitHub](https://github.com/ethanjones1132-lab/Prometheus) · [Issues](https://github.com/ethanjones1132-lab/Prometheus/issues)

</div>
