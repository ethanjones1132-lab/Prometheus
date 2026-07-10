# Agent orchestrator — fan-out to available estimators (plan §5, §7 Phase 2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Currently ships two *real* estimators grounded in available data:
#   1. technical — yfinance spot/vol → P(S_T > K)
#   2. contract_tape — Kalshi mid series from request context
#
# Explicitly NOT shipped yet (no honest data path right now):
#   - macro (needs EconDB / release calendars — not installed)
#   - news / sentiment (need LLM + news feeds — no keys/data here)
#   - valuation / fundamentals (need fundamentals DB for company events)
#   - risk / portfolio / explainability (shape sizing/reporting, not p_model)

from __future__ import annotations

import time
from datetime import datetime, timezone

from fincept_sidecar.schemas import (
    AgentSignal,
    CatalystEvent,
    MarketOpinionRequest,
    MarketOpinionResponse,
)

from . import contract_tape, technical


async def collect_market_opinion(req: MarketOpinionRequest) -> MarketOpinionResponse:
    t0 = time.perf_counter()
    signals: list[AgentSignal] = []

    signals.append(await technical.estimate(req))
    signals.append(await contract_tape.estimate(req))

    # Placeholder no-opinion rows for agents that exist in the plan but have
    # no live data path here — honest None, never a fake probability.
    for name, reason in (
        (
            "macro",
            "Macro agent requires EconDB (or equivalent) release series; not available in this sidecar build.",
        ),
        (
            "news",
            "News agent requires a resolution-aware news feed / LLM path; not wired.",
        ),
        (
            "sentiment",
            "Sentiment agent requires social/news sentiment feeds; not wired.",
        ),
    ):
        signals.append(
            AgentSignal(
                agent=name,
                probability=None,
                confidence=0.0,
                rationale=reason,
                inputs_used=[],
                caveats=["data_source_unavailable"],
            )
        )

    catalysts: list[CatalystEvent] = []
    # Surface close_time as a known catalyst boundary (not a news scrape).
    catalysts.append(
        CatalystEvent(
            description="Contract close / resolution window end (from Kalshi close_time)",
            occurs_at=req.close_time,
            source="kalshi:close_time",
        )
    )

    elapsed_ms = int((time.perf_counter() - t0) * 1000)
    return MarketOpinionResponse(
        signals=signals,
        catalysts=catalysts,
        elapsed_ms=elapsed_ms,
    )


# Silence unused import warning if timezone needed later
_ = datetime, timezone
