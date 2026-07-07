# SPDX-License-Identifier: AGPL-3.0-or-later
"""Real-subprocess handshake test (plan §7 Phase 1 exit criteria).

Spawns main.py exactly as the Rust FinceptBridge will: random token in env,
FINCEPT_PORT=0, read `FINCEPT_READY port=<n>` from stdout, then make an
authed request against the announced port.
"""

import os
import secrets
import subprocess
import sys
import time
from pathlib import Path

import httpx
import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
READY_TIMEOUT_S = 20


@pytest.fixture()
def sidecar_process():
    token = secrets.token_hex(16)
    env = dict(os.environ, FINCEPT_TOKEN=token, FINCEPT_PORT="0", FINCEPT_LOG_LEVEL="warning")
    proc = subprocess.Popen(
        [sys.executable, str(REPO_ROOT / "main.py")],
        env=env,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        yield proc, token
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


def read_ready_port(proc: subprocess.Popen) -> int:
    deadline = time.monotonic() + READY_TIMEOUT_S
    while time.monotonic() < deadline:
        line = proc.stdout.readline()
        if not line:
            if proc.poll() is not None:
                raise AssertionError(f"sidecar exited early: {proc.stderr.read()}")
            continue
        if line.startswith("FINCEPT_READY port="):
            return int(line.strip().split("=", 1)[1])
    raise AssertionError("no FINCEPT_READY line within timeout")


def test_handshake_announces_port_and_serves_authed_requests(sidecar_process):
    proc, token = sidecar_process
    port = read_ready_port(proc)
    assert 1024 <= port <= 65535

    base = f"http://127.0.0.1:{port}"
    with httpx.Client(timeout=10) as client:
        # Unauthenticated request is rejected...
        assert client.get(f"{base}/api/v1/health").status_code == 401
        # ...token gets through.
        r = client.get(f"{base}/api/v1/health", headers={"Authorization": f"Bearer {token}"})
        assert r.status_code == 200
        assert r.json()["status"] == "ok"


def test_missing_token_exits_nonzero():
    env = {k: v for k, v in os.environ.items() if k != "FINCEPT_TOKEN"}
    proc = subprocess.run(
        [sys.executable, str(REPO_ROOT / "main.py")],
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=15,
    )
    assert proc.returncode == 2
    assert "FINCEPT_TOKEN" in proc.stderr
