#!/usr/bin/env python3
"""Build the Fincept sidecar as a single-file executable and stage it for Tauri externalBin.

Usage:
    python scripts/build_fincept_sidecar.py
    python scripts/build_fincept_sidecar.py --dry-run
    python scripts/build_fincept_sidecar.py --check-env

Requires PyInstaller in the fincept-sidecar virtualenv:
    cd fincept-sidecar
    .venv\\Scripts\\activate   # Windows
    pip install pyinstaller
    cd ..
    python scripts/build_fincept_sidecar.py

The resulting executable is copied to:
    kalshi-monster/src-tauri/binaries/fincept-sidecar-<target-triple>[.exe]

Tauri strips the target triple at bundle time and places the binary next to the app executable.
"""

from __future__ import annotations

import os
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


def sidecar_python(repo_root: Path) -> Path:
    """Always use the fincept-sidecar venv — never the caller's interpreter (e.g. Hermes cron)."""
    sidecar_dir = repo_root / "fincept-sidecar"
    if platform.system().lower() == "windows":
        py = sidecar_dir / ".venv" / "Scripts" / "python.exe"
    else:
        py = sidecar_dir / ".venv" / "bin" / "python"
    if not py.is_file():
        raise FileNotFoundError(
            f"fincept-sidecar venv python not found: {py}\n"
            "Create the venv under fincept-sidecar/ and pip install -e '.[market,dev]' plus pyinstaller."
        )
    return py


def isolated_build_env(sidecar_dir: Path) -> dict[str, str]:
    """Drop Hermes/cron PYTHONPATH entries that poison PyInstaller (wrong numpy/torch wheels)."""
    env = os.environ.copy()
    raw = env.get("PYTHONPATH", "")
    if raw:
        cleaned: list[str] = []
        for part in raw.split(os.pathsep):
            if not part:
                continue
            norm = part.replace("\\", "/").lower()
            if "hermes-agent" in norm or "/hermes/hermes-agent/" in norm:
                continue
            cleaned.append(part)
        if cleaned:
            env["PYTHONPATH"] = os.pathsep.join(cleaned)
        else:
            env.pop("PYTHONPATH", None)
    env["VIRTUAL_ENV"] = str(sidecar_dir / ".venv")
    return env


def _self_test() -> int:
    sidecar = Path("/tmp/fincept-sidecar-test")
    env = {
        "PYTHONPATH": os.pathsep.join(
            [
                r"C:\Users\ethan\AppData\Local\hermes\hermes-agent",
                r"C:\Users\ethan\AppData\Local\hermes\hermes-agent\venv\Lib\site-packages",
                r"D:\safe\lib",
            ]
        )
    }
    os.environ.update(env)
    cleaned = isolated_build_env(sidecar)
    assert "PYTHONPATH" in cleaned and cleaned["PYTHONPATH"] == r"D:\safe\lib", cleaned
    print("self-test ok: hermes PYTHONPATH stripped")
    return 0


def check_sidecar_env(repo_root: Path) -> int:
    py = sidecar_python(repo_root)
    sidecar_dir = repo_root / "fincept-sidecar"
    env = isolated_build_env(sidecar_dir)
    probes = [
        ("import fincept_sidecar", "fincept_sidecar package"),
        ("import pydantic", "pydantic"),
        ("import uvicorn", "uvicorn"),
    ]
    for code, label in probes:
        r = subprocess.run(
            [str(py), "-c", code],
            cwd=sidecar_dir,
            env=env,
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            print(f"check-env FAIL: {label}\n{r.stderr or r.stdout}", file=sys.stderr)
            return 1
        print(f"check-env ok: {label}")
    r = subprocess.run(
        [str(py), "-c", "import PyInstaller"],
        cwd=sidecar_dir,
        env=env,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        print(
            "check-env FAIL: PyInstaller not installed in fincept-sidecar venv "
            "(pip install -e '.[bundle]' in fincept-sidecar/)",
            file=sys.stderr,
        )
        if r.stderr or r.stdout:
            print(r.stderr or r.stdout, file=sys.stderr)
        return 1
    print("check-env ok: PyInstaller")
    print(f"check-env ok: interpreter={py}")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()
    dry_run = "--dry-run" in sys.argv
    check_env = "--check-env" in sys.argv
    repo_root = Path(__file__).resolve().parent.parent
    sidecar_dir = repo_root / "fincept-sidecar"
    entrypoint = sidecar_dir / "main.py"
    if not entrypoint.is_file():
        print(f"error: sidecar entrypoint not found: {entrypoint}", file=sys.stderr)
        return 1

    if check_env:
        return check_sidecar_env(repo_root)

    triple = target_triple()
    output_name = exe_name(triple)
    binaries_dir = repo_root / "kalshi-monster" / "src-tauri" / "binaries"
    binaries_dir.mkdir(parents=True, exist_ok=True)

    if dry_run:
        dest = binaries_dir / output_name
        print(f"dry-run ok: entrypoint={entrypoint}")
        print(f"dry-run ok: target_triple={triple}")
        print(f"dry-run ok: staged_path={dest}")
        try:
            py = sidecar_python(repo_root)
            print(f"dry-run ok: sidecar_python={py}")
        except FileNotFoundError as e:
            print(f"dry-run warn: {e}", file=sys.stderr)
        return 0

    py = sidecar_python(repo_root)
    env = isolated_build_env(sidecar_dir)

    dist_dir = sidecar_dir / "dist"
    if dist_dir.exists():
        shutil.rmtree(dist_dir)

    print(f"Building Fincept sidecar for {triple} with {py} ...")
    cmd = [
        str(py),
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
    subprocess.run(cmd, cwd=sidecar_dir, env=env, check=True)

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