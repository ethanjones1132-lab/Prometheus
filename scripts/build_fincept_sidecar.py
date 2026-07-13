#!/usr/bin/env python3
"""Build the Fincept sidecar as a single-file executable and stage it for Tauri externalBin.

Usage:
    python scripts/build_fincept_sidecar.py

Requires PyInstaller in the fincept-sidecar virtualenv:
    cd fincept-sidecar
    source .venv/bin/activate  # or .venv\\Scripts\\activate on Windows
    pip install pyinstaller
    cd ..
    python scripts/build_fincept_sidecar.py

The resulting executable is copied to:
    kalshi-monster/src-tauri/binaries/fincept-sidecar-<target-triple>[.exe]

Tauri strips the target triple at bundle time and places the binary next to the app executable.
"""

from __future__ import annotations

import platform
import shutil
import subprocess
import sys
from pathlib import Path


def target_triple() -> str:
    machine = platform.machine().lower()
    system = platform.system().lower()
    if system == "windows":
        return "x86_64-pc-windows-msvc"
    if system == "darwin":
        return "aarch64-apple-darwin" if machine == "arm64" else "x86_64-apple-darwin"
    if system == "linux":
        return "aarch64-unknown-linux-gnu" if machine in ("arm64", "aarch64") else "x86_64-unknown-linux-gnu"
    raise RuntimeError(f"Unsupported build platform: {system}/{machine}")


def exe_name(triple: str) -> str:
    suffix = ".exe" if "windows" in triple else ""
    return f"fincept-sidecar-{triple}{suffix}"


def main() -> int:
    dry_run = "--dry-run" in sys.argv
    repo_root = Path(__file__).resolve().parent.parent
    sidecar_dir = repo_root / "fincept-sidecar"
    entrypoint = sidecar_dir / "main.py"
    if not entrypoint.is_file():
        print(f"error: sidecar entrypoint not found: {entrypoint}", file=sys.stderr)
        return 1

    triple = target_triple()
    output_name = exe_name(triple)
    binaries_dir = repo_root / "kalshi-monster" / "src-tauri" / "binaries"
    binaries_dir.mkdir(parents=True, exist_ok=True)

    if dry_run:
        dest = binaries_dir / output_name
        print(f"dry-run ok: entrypoint={entrypoint}")
        print(f"dry-run ok: target_triple={triple}")
        print(f"dry-run ok: staged_path={dest}")
        return 0

    # Tauri externalBin expects the base name in the config and a file named
    # <base>-<target-triple>[.exe] in src-tauri/binaries/.
    tauri_bin_name = f"fincept-sidecar-{triple}{'.exe' if 'windows' in triple else ''}"

    dist_dir = sidecar_dir / "dist"
    if dist_dir.exists():
        shutil.rmtree(dist_dir)

    print(f"Building Fincept sidecar for {triple} ...")
    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--onefile",
        "--name",
        "fincept-sidecar",
        "--distpath",
        str(dist_dir),
        "--workpath",
        str(sidecar_dir / "build"),
        "--specpath",
        str(sidecar_dir),
        str(entrypoint),
    ]
    subprocess.run(cmd, cwd=sidecar_dir, check=True)

    built = dist_dir / ("fincept-sidecar.exe" if "windows" in triple else "fincept-sidecar")
    if not built.is_file():
        print(f"error: PyInstaller did not produce {built}", file=sys.stderr)
        return 1

    dest = binaries_dir / output_name
    shutil.copy2(built, dest)
    print(f"Staged sidecar for Tauri externalBin: {dest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
