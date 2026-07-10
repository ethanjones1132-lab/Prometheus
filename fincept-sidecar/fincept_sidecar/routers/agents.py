# fincept-sidecar — agent opinion endpoints (plan §7 Phase 2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later

from __future__ import annotations

from fastapi import APIRouter

from agents.orchestrator import collect_market_opinion
from fincept_sidecar.schemas import MarketOpinionRequest, MarketOpinionResponse

router = APIRouter(tags=["agents"])


@router.post("/market-opinion", response_model=MarketOpinionResponse)
async def market_opinion(req: MarketOpinionRequest) -> MarketOpinionResponse:
    """Run available probability estimators and return AgentSignal list.

    Money path stays in Rust: this endpoint never sizes, never trades.
    """
    return await collect_market_opinion(req)
