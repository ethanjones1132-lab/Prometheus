# fincept-sidecar — process entrypoint (plan §7 Phase 1, §10.1)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Startup handshake:
#   1. Parent (Rust FinceptBridge) spawns this with env FINCEPT_TOKEN (random
#      per launch) and optionally FINCEPT_PORT (0 or unset = OS-assigned).
#   2. We bind + listen on 127.0.0.1 ourselves, print `FINCEPT_READY port=<n>`
#      on stdout, then hand the live socket to uvicorn.
#   3. Parent reads the line (30 s timeout) and starts issuing bearer-token
#      requests. Early connections are safe: the socket is already listening,
#      so they queue in the accept backlog until uvicorn takes over.
#
# This replaces the plan's sketch (which reached into uvicorn server internals
# from a startup hook — fragile across uvicorn versions) with a socket we own.

from __future__ import annotations

import os
import socket
import sys

import uvicorn

from fincept_sidecar.app import create_app_from_env


def main() -> int:
    if not os.environ.get("FINCEPT_TOKEN"):
        print("fatal: FINCEPT_TOKEN is required (generated per-launch by the parent)", file=sys.stderr)
        return 2

    port = int(os.environ.get("FINCEPT_PORT", "0"))

    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", port))  # 127.0.0.1 only — never 0.0.0.0 (plan §10.2)
    sock.listen(128)
    actual_port = sock.getsockname()[1]

    app = create_app_from_env()
    config = uvicorn.Config(app, log_level=os.environ.get("FINCEPT_LOG_LEVEL", "info"))
    server = uvicorn.Server(config)

    print(f"FINCEPT_READY port={actual_port}", flush=True)
    server.run(sockets=[sock])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
