# fincept-sidecar — agent opinion endpoints (plan §7 Phase 2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later

from __future__ import annotations

from fastapi import APIRouter

from agents.asset_continuous import estimate_asset_signal
from agents.orchestrator import collect_market_opinion
from fincept_sidecar.schemas import (
    AssetSignal,
    AssetSignalRequest,
    MarketOpinionRequest,
    MarketOpinionResponse,
)

router = APIRouter(tags=["agents"])


@router.post("/market-opinion", response_model=MarketOpinionResponse)
async def market_opinion(req: MarketOpinionRequest) -> MarketOpinionResponse:
    """Run available probability estimators and return AgentSignal list.

    Money path stays in Rust: this endpoint never sizes, never trades.
    """
    return await collect_market_opinion(req)


@router.post("/asset-signal", response_model=AssetSignal)
async def asset_signal(req: AssetSignalRequest) -> AssetSignal:
    """Continuous-payoff AssetSignal (plan §14.4). Gated until binary calibration is OPEN.

    Returns expected_excess_return=None when gated or data is insufficient.
    """
    closes = req.closes if req.closes else None
    return estimate_asset_signal(
        req.ticker,
        horizon_days=req.horizon_days,
        calibration_gate_open=req.calibration_gate_open,
        closes=closes,
    )
