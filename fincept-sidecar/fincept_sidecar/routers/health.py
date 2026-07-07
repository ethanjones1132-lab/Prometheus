# fincept-sidecar — health & version endpoints (plan Appendix A)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later

from __future__ import annotations

import os
import time

from fastapi import APIRouter, Request

from .. import __version__
from ..schemas import HealthResponse, VersionResponse

router = APIRouter(tags=["meta"])


@router.get("/health", response_model=HealthResponse)
async def health(request: Request) -> HealthResponse:
    return HealthResponse(
        status="ok",
        uptime_seconds=time.monotonic() - request.app.state.started_at,
    )


@router.get("/version", response_model=VersionResponse)
async def version() -> VersionResponse:
    # CI embeds the git SHA at build time (plan §3 Rule 3: every shipped
    # binary must be traceable to a public tag).
    return VersionResponse(version=__version__, git_sha=os.environ.get("FINCEPT_GIT_SHA", "dev"))
