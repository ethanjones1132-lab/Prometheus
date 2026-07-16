# AGPL sidecar isolation — Sprint 7.1

**Status:** Process + tooling ready; public split is an operator action (one-time).  
**Constraint (plan §3 Rule 1):** Fincept-derived code may live only in an AGPL process  
(`fincept-sidecar`). `kalshi-monster` must stay free of Fincept-derived sources.

## Current layout

```text
kalshi-build/                 # private monorepo (this workspace)
  fincept-sidecar/            # AGPL analysis process (HTTP only)
  kalshi-monster/             # proprietary app (Rust/Tauri) — money path
```

At runtime, Kalshi Monster talks to the sidecar **only** over authenticated HTTP  
(`FinceptBridge` → `POST /api/v1/agents/*`). No Python embedding.

## Public split checklist

1. **Create** a public GitHub repo (suggested name: `fincept-sidecar` or `kalshi-fincept-sidecar`).
2. **Push history** of `fincept-sidecar/` only (subtree or filter-repo):

   ```bash
   # From monorepo root (example with git subtree split)
   git subtree split --prefix=fincept-sidecar -b sidecar-public
   cd /tmp && git clone --branch sidecar-public <monorepo-url> fincept-sidecar-public
   cd fincept-sidecar-public
   git remote set-url origin git@github.com:<org>/fincept-sidecar.git
   git push -u origin sidecar-public:main
   ```

   Prefer `git filter-repo --path fincept-sidecar --path-rename fincept-sidecar/:`  
   if you need a clean root without monorepo noise.

3. **Pin** the released sidecar commit SHA in:
   - `kalshi-monster/src-tauri/binaries/README.md`
   - App Settings → Fincept card (source offer link)
   - Installer / release notes

4. **License files** already present: `fincept-sidecar/LICENSE` (AGPL-3.0), `NOTICE`.

5. **CI** in the public repo: `uv run pytest` (or `python -m pytest`).

6. **After split:** monorepo may keep a submodule or vendored release binary  
   (`src-tauri/binaries/fincept-sidecar-*.exe`) without re-importing AGPL sources  
   into the proprietary tree.

## Allowed in monorepo after split

| Path | OK? |
|------|-----|
| Bundled sidecar **binary** for Tauri `externalBin` | Yes |
| HTTP schemas mirrored in Rust (`AgentSignal`) | Yes (contract only) |
| Fincept Python sources inside `kalshi-monster/` | **No** |
| Copying Fincept algorithms into Rust | **No** |

## Modules with real data paths (Sprint 7.2)

| Agent | Data path | Status |
|-------|-----------|--------|
| technical | yfinance | Live |
| contract_tape | Kalshi mids from Rust | Live |
| news | web_snippets from Rust | Live |
| macro | FRED API (`FRED_API_KEY`) | Live |
| sentiment | — | Stub null |
| fundamentals / valuation | — | Stub null (need DB) |
| asset continuous | gated AssetSignal | Scaffold (7.3) |

Do **not** port Fincept EconDB / fundamentals modules until this split is public  
and a non-hallucinating series mapping exists.

## Operator verification

```bash
# Sidecar unit tests
cd fincept-sidecar && python -m pytest -q

# Packaging contract (Rust)
cd kalshi-monster/src-tauri && cargo test --lib fincept_bridge::tests::release_conf
```
