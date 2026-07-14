# Fincept Sidecar Binary

This directory holds the Fincept sidecar executable for Tauri `externalBin` bundling.

## Layout

Tauri expects a file named:

```text
fincept-sidecar-<target-triple>[.exe]
```

For example, on 64-bit Windows:

```text
fincept-sidecar-x86_64-pc-windows-msvc.exe
```

At bundle time Tauri strips the target triple and places the binary next to the app executable as `fincept-sidecar(.exe)`.

## Build

From the repo root, with the fincept-sidecar virtualenv active and PyInstaller installed:

```bash
python scripts/build_fincept_sidecar.py
```

Validate layout without building (CI / maintenance passes):

```bash
python scripts/build_fincept_sidecar.py --dry-run
python scripts/build_fincept_sidecar.py --check-env
```

**Cron / Hermes hosts:** parent `PYTHONPATH` may include Hermes agent site-packages (wrong numpy wheels). The build script strips those entries and always uses `fincept-sidecar/.venv/Scripts/python.exe`. Install bundle deps with uv:

```bash
cd fincept-sidecar
uv pip install --python .venv/Scripts/python.exe -e ".[market,bundle]"
```

This produces the staged executable in this directory.

## Release builds

The base `tauri.conf.json` does **not** list the sidecar so that `cargo check` and `tauri dev` work without a staged binary. For release bundles, run:

```bash
cd kalshi-monster/src-tauri
tauri build --config tauri.conf.release.json
```

This merges `tauri.conf.release.json`, which declares the `externalBin` entry.
