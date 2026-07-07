# fincept-sidecar — FastAPI application factory (plan §7 Phase 1, §10.1–10.2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later

from __future__ import annotations

import hmac
import os
import time

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from .routers import health, market

# Paths that intentionally bypass auth: none. Any local process can reach a
# localhost port, so every request must carry the per-launch bearer token
# (plan §10.2). The Rust supervisor knows the token and health-checks with it.


def create_app(token: str) -> FastAPI:
    """Build the app with a per-launch bearer token.

    A factory (rather than module-level app state) so tests can construct
    instances with known tokens and no environment coupling.
    """
    if not token:
        raise ValueError("fincept-sidecar requires a non-empty auth token")

    app = FastAPI(
        title="fincept-sidecar",
        version="0.1.0",
        description=(
            "Analysis sidecar for Kalshi Monster. Owns analysis, never money: "
            "no bankroll state, no Kalshi credentials, no order placement."
        ),
    )
    app.state.started_at = time.monotonic()
    expected = f"Bearer {token}"

    @app.middleware("http")
    async def require_bearer_token(request: Request, call_next):
        supplied = request.headers.get("authorization", "")
        # Constant-time comparison; a plain == on secrets invites timing probes.
        if not hmac.compare_digest(supplied.encode(), expected.encode()):
            return JSONResponse(status_code=401, content={"detail": "unauthorized"})
        return await call_next(request)

    app.include_router(health.router, prefix="/api/v1")
    app.include_router(market.router, prefix="/api/v1/market")
    return app


def create_app_from_env() -> FastAPI:
    """Entry used by main.py: token comes from the parent process's env."""
    return create_app(os.environ.get("FINCEPT_TOKEN", ""))
