# Contract-tape technical agent — signals from the Kalshi contract's own mids (plan §5.1 #2 novel role)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Data source: req.context["contract_mids"] supplied by the Rust core from
# Kalshi orderbook/mids or price-tracker snapshots — NOT invented here.
# If context lacks a real series, probability=None.

from __future__ import annotations

import math
from datetime import datetime, timezone
from typing import Any

from fincept_sidecar.schemas import AgentSignal, DataRef, MarketOpinionRequest


def clamp_prob(p: float) -> float:
    return max(0.01, min(0.99, p))


def _as_float_series(raw: Any) -> list[float]:
    if not isinstance(raw, list):
        return []
    out: list[float] = []
    for item in raw:
        if isinstance(item, (int, float)):
            p = float(item)
        elif isinstance(item, dict):
            for k in ("mid", "yes_mid", "price", "p"):
                if k in item and item[k] is not None:
                    try:
                        p = float(item[k])
                        break
                    except (TypeError, ValueError):
                        p = None  # type: ignore[assignment]
                else:
                    p = None  # type: ignore[assignment]
            else:
                continue
            if p is None:
                continue
        else:
            continue
        if 0.0 < p < 1.0:
            out.append(p)
    return out


def estimate_sync(req: MarketOpinionRequest) -> AgentSignal:
    """Momentum + longshot-bias adjustment on the contract mid path.

    - Momentum: if recent mids rose, slightly raise P(YES); fall → lower.
    - Longshot bias: extreme mids (<0.15 or >0.85) are pulled toward 0.5
      modestly — a well-documented prediction-market bias, applied as a
      *small* correction with low confidence, not a dominant signal.
    """
    ctx = req.context or {}
    mids = _as_float_series(ctx.get("contract_mids"))
    # Also accept a single latest mid from yes_bid/yes_ask when series missing.
    market_mid = None
    if req.yes_bid is not None and req.yes_ask is not None:
        market_mid = 0.5 * (float(req.yes_bid) + float(req.yes_ask))

    if len(mids) < 3 and market_mid is None:
        return AgentSignal(
            agent="contract_tape",
            probability=None,
            confidence=0.0,
            rationale=(
                "No contract mid series in context (need context.contract_mids with ≥3 "
                "points in (0,1)) and no usable yes bid/ask. No opinion."
            ),
            inputs_used=[],
            caveats=["missing_contract_mids"],
        )

    caveats: list[str] = []
    inputs: list[DataRef] = []
    now = datetime.now(timezone.utc)

    if len(mids) >= 3:
        latest = mids[-1]
        older = mids[0]
        # Log-odds momentum over the series window.
        def logit(p: float) -> float:
            p = clamp_prob(p)
            return math.log(p / (1.0 - p))

        delta = logit(latest) - logit(older)
        # Map delta to a small probability shift: tanh scale.
        mom_shift = 0.08 * math.tanh(delta)  # at most ±8¢
        p_mom = clamp_prob(latest + mom_shift)

        # Longshot pull: shrink extremes 15% toward 0.5.
        if latest < 0.15 or latest > 0.85:
            p_longshot = 0.85 * latest + 0.15 * 0.5
            caveats.append("longshot_bias_adjustment")
        else:
            p_longshot = latest

        p = clamp_prob(0.6 * p_mom + 0.4 * p_longshot)
        conf = min(0.55, 0.20 + 0.03 * len(mids))
        inputs.append(
            DataRef(
                source=f"kalshi_context:contract_mids:n={len(mids)}",
                fetched_at=now,
            )
        )
        rationale = (
            f"Contract-tape signal from {len(mids)} mid points supplied by Rust context "
            f"(Kalshi). Latest mid={latest:.3f}, series-start={older:.3f}, "
            f"logit-momentum shift={mom_shift:+.3f} → p={p:.3f}. "
            "This is a weak secondary signal (low confidence by design)."
        )
    else:
        assert market_mid is not None
        latest = clamp_prob(market_mid)
        if latest < 0.15 or latest > 0.85:
            p = clamp_prob(0.85 * latest + 0.15 * 0.5)
            caveats.append("longshot_bias_adjustment_single_mid")
            conf = 0.15
        else:
            # No series → refuse to invent momentum; longshot-only on mid is weak.
            return AgentSignal(
                agent="contract_tape",
                probability=None,
                confidence=0.0,
                rationale=(
                    f"Only a single market mid ({latest:.3f}) available; no mid history "
                    "for momentum. Returning no opinion rather than a one-point claim."
                ),
                inputs_used=[
                    DataRef(source="kalshi_request:yes_bid_ask_mid", fetched_at=now),
                ],
                caveats=["single_mid_insufficient"],
            )
        inputs.append(DataRef(source="kalshi_request:yes_bid_ask_mid", fetched_at=now))
        rationale = (
            f"Single-mid longshot adjustment only: mid={latest:.3f} → p={p:.3f}. "
            "Very low confidence; prefer series when price_tracker supplies history."
        )

    # Never claim independence from the market mid — this agent reads the tape.
    caveats.append("tape_correlated_with_p_market")

    return AgentSignal(
        agent="contract_tape",
        probability=p,
        confidence=conf,
        rationale=rationale,
        inputs_used=inputs,
        caveats=caveats,
    )


async def estimate(req: MarketOpinionRequest) -> AgentSignal:
    # Pure CPU over context — async for orchestrator uniformity.
    return estimate_sync(req)
