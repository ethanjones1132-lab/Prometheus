#!/usr/bin/env python3
"""Merge tauri.conf.release.json overlay into base config for Windows release builds.

Why: `cargo tauri build --config tauri.conf.release.json` treats the overlay as a
full config and drops base fields. On MSYS/git-bash, `beforeBuildCommand`'s
relative `cd ../src-ui` also fails. Pre-build the frontend, then use this merge
with an empty beforeBuildCommand.

Usage (from repo root):
    python scripts/merge_release_config.py
    # writes kalshi-monster/src-tauri/tauri.conf.release.merged.json (gitignored)

    python scripts/merge_release_config.py --no-external-bin
    # writes tauri.conf.release.nobuild.json without externalBin (sidecar optional)

See: skills/cron-maintenance/kalshi-cron/references/release-build-2026-07-16.md
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def deep_merge(base: dict, overlay: dict) -> dict:
    out = dict(base)
    for k, v in overlay.items():
        if k in out and isinstance(out[k], dict) and isinstance(v, dict):
            out[k] = deep_merge(out[k], v)
        else:
            out[k] = v
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-external-bin",
        action="store_true",
        help="Clear bundle.externalBin (build without staged fincept-sidecar)",
    )
    parser.add_argument(
        "--src-tauri",
        type=Path,
        default=None,
        help="Path to src-tauri (default: kalshi-monster/src-tauri relative to repo root)",
    )
    args = parser.parse_args()

    repo = Path(__file__).resolve().parent.parent
    src_tauri = args.src_tauri or (repo / "kalshi-monster" / "src-tauri")
    base_path = src_tauri / "tauri.conf.json"
    overlay_path = src_tauri / "tauri.conf.release.json"
    if not base_path.is_file() or not overlay_path.is_file():
        print(f"ERROR: missing conf under {src_tauri}", file=sys.stderr)
        return 1

    base = json.loads(base_path.read_text(encoding="utf-8"))
    overlay = json.loads(overlay_path.read_text(encoding="utf-8"))
    merged = deep_merge(base, overlay)
    merged.setdefault("build", {})["beforeBuildCommand"] = ""

    if args.no_external_bin:
        merged.setdefault("bundle", {})["externalBin"] = []
        out_path = src_tauri / "tauri.conf.release.nobuild.json"
    else:
        out_path = src_tauri / "tauri.conf.release.merged.json"

    out_path.write_text(json.dumps(merged, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {out_path}")
    print("Next: cargo tauri build --config", out_path.name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
