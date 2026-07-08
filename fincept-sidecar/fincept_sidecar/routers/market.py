# fincept-sidecar — market data endpoints (plan §7 Phase 1, Appendix A)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Thin ORIGINAL wrappers over yfinance (Apache-2.0). Deliberately not derived
# from Fincept code: commodity data plumbing stays on the permissive path so
# the AGPL-critical surface stays small (plan §3 Rule 5b).

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from ..engines import market_data, tracker

router = APIRouter(tags=["market"])


@router.get("/tracker")
async def tracker_all() -> dict:
    return await tracker.get_tracker(None)


@router.get("/tracker/{category}")
async def tracker_category(category: str) -> dict:
    if category not in tracker.VALID_CATEGORIES:
        raise HTTPException(
            status_code=400,
            detail=f"unknown category {category!r}; expected one of {sorted(tracker.VALID_CATEGORIES)}",
        )
    return await tracker.get_tracker(category)


@router.get("/snapshot")
async def chat_snapshot() -> dict:
    """Compact cross-asset snapshot for chat context injection."""
    return await tracker.get_chat_snapshot()


@router.get("/price/{ticker}")
async def price(ticker: str) -> dict:
    quote = await market_data.get_quote(ticker)
    if quote is None:
        raise HTTPException(status_code=404, detail=f"no data for {ticker!r}")
    return quote


@router.get("/history/{ticker}")
async def history(ticker: str, period: str = "1mo", interval: str = "1d") -> dict:
    bars = await market_data.get_history(ticker, period=period, interval=interval)
    if bars is None:
        raise HTTPException(status_code=404, detail=f"no history for {ticker!r}")
    return bars