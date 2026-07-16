# fincept-sidecar

Analysis sidecar for [Kalshi Monster], per the integration plan
(`kalshi-monster/docs/fincept-integration-plan.md`, §2, §3, §7 Phase 1).

The sidecar owns **analysis**: external data fetching, agent probability
estimates, macro indicators, portfolio analytics. It is stateless apart from
its caches, holds no bankroll state, and never talks to Kalshi's trading
endpoints. The Rust core owns **decisions and money**.

## Status (2026-07)

Implemented:

- FastAPI app with per-launch bearer-token auth (constant-time compare)
- Ephemeral-port startup handshake: binds `127.0.0.1:0`, prints
  `FINCEPT_READY port=<n>` on stdout for the parent process
- `GET /api/v1/health`, `GET /api/v1/version`
- `POST /api/v1/agents/market-opinion` — technical, contract_tape, news, macro
  (depth tiers: quick / standard / deep)
- `POST /api/v1/agents/asset-signal` — continuous book scaffold; **gated** until
  binary calibration is OPEN (§14.4)
- Pydantic contracts: `AgentSignal`, `MarketOpinionRequest/Response`,
  `CatalystEvent`, `AssetSignal` / `AssetSignalRequest`
- Market data via yfinance (optional); FRED for macro when `FRED_API_KEY` set
- Tests: auth, schemas, technical math, news/macro null paths, asset gate

Not yet: sentiment/fundamentals/valuation with real DBs; full Fincept EconDB
extract (requires public AGPL split — see monorepo `docs/AGPL-SIDECAR-SPLIT.md`).

## Run

```bash
pip install -e ".[dev,market]"
FINCEPT_TOKEN=dev-token python main.py            # ephemeral port, announced on stdout
FINCEPT_TOKEN=dev-token FINCEPT_PORT=8991 python main.py   # fixed port (dev only)

curl -H "Authorization: Bearer dev-token" http://127.0.0.1:<port>/api/v1/health
pytest
```

## Handshake design note

The plan's §7 sketch announced the port from a FastAPI startup hook by
reaching into uvicorn's server internals — fragile across uvicorn versions.
This implementation binds the socket itself (`127.0.0.1`, port 0 = OS-assigned),
starts listening, prints `FINCEPT_READY port=<n>`, and hands the socket to
uvicorn via `Server.serve(sockets=[...])`. Announcing before uvicorn's accept
loop starts is safe: the socket is already bound and listening, so early
connections queue in the backlog.

## Licensing rules (plan §3 — read before contributing)

1. This repo is **AGPL-3.0** (LICENSE, NOTICE). It is the *only* place
   Fincept-derived code may live.
2. `kalshi-monster` must contain zero lines of Fincept-derived code — no
   copied Python, no translated algorithms, no copied prompt templates. Its
   only knowledge of this service is the HTTP API contract.
3. Commodity data plumbing (yfinance, empyrical, EconDB REST, standard TA)
   is written as **original code against permissively-licensed libraries**
   (`engines/`), keeping the AGPL-critical surface small (§3 Rule 5b).
4. Before any public distribution of Kalshi Monster bundles that include this
   sidecar: pin the shipped commit publicly, link it from the app's about
   screen, and get a legal review (§3 Rule 3).
5. **This directory currently sits inside the `kalshi-build` working folder
   for convenience. Before any Fincept code is added, split it into its own
   public repository** (plan §3 Rule 1) and remove it from any private repo's
   history going forward.

## Interface stability

`fincept_sidecar/schemas.py` is the source of truth for the wire contract.
The Rust side (`kalshi-monster/src-tauri/src/edge_engine/mod.rs`,
`AgentSignal`) mirrors it field-for-field; change them in lock-step. CI in
both repos should eventually pin the exported OpenAPI schema (plan §11).
