# Continuous AssetSignal path (plan §14.4 / Sprint 7.3)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Gated until binary calibration matures (gate OPEN, ≥200 resolved).
# When ungated by caller context, may use yfinance history for a crude
# momentum prior — still returns expected_excess_return=None when data is thin.

from __future__ import annotations

import math
from datetime import datetime, timezone
from typing import Any

from fincept_sidecar.schemas import AssetSignal, DataRef


def _annualized_vol(closes: list[float]) -> float | None:
    if len(closes) < 15:
        return None
    rets = []
    for i in range(1, len(closes)):
        a, b = closes[i - 1], closes[i]
        if a > 0 and b > 0:
            rets.append(math.log(b / a))
    if len(rets) < 10:
        return None
    mean = sum(rets) / len(rets)
    var = sum((r - mean) ** 2 for r in rets) / len(rets)
    return math.sqrt(var) * math.sqrt(252.0)


def estimate_asset_signal(
    ticker: str,
    horizon_days: int = 21,
    *,
    calibration_gate_open: bool = False,
    closes: list[float] | None = None,
) -> AssetSignal:
    """
    Honest continuous-payoff signal.

    - If calibration gate is not open: always no opinion (binary first).
    - If open but no price history: no opinion.
    - If history present: weak momentum excess return + realized vol.
    """
    if not calibration_gate_open:
        return AssetSignal(
            agent="asset_momentum",
            ticker=ticker,
            horizon_days=max(1, horizon_days),
            expected_excess_return=None,
            return_vol=0.20,
            confidence=0.0,
            rationale=(
                "AssetSignal path gated until binary forecast calibration gate is OPEN "
                "(≥200 resolved, Brier/p&l criteria). No continuous opinion yet."
            ),
            inputs_used=[],
        )

    if not closes or len(closes) < 21:
        return AssetSignal(
            agent="asset_momentum",
            ticker=ticker,
            horizon_days=max(1, horizon_days),
            expected_excess_return=None,
            return_vol=0.20,
            confidence=0.0,
            rationale=f"Insufficient history for {ticker}; refusing to invent expected return.",
            inputs_used=[],
        )

    vol = _annualized_vol(closes) or 0.25
    a, b = closes[-21], closes[-1]
    if a <= 0 or b <= 0:
        mu = None
    else:
        # Trailing 20d return annualized, capped ±30%
        r = math.log(b / a) * (252.0 / 20.0)
        mu = max(-0.30, min(0.30, r))

    return AssetSignal(
        agent="asset_momentum",
        ticker=ticker,
        horizon_days=max(1, horizon_days),
        expected_excess_return=mu,
        return_vol=max(0.05, min(1.5, vol)),
        confidence=0.15 if mu is not None else 0.0,
        rationale=(
            f"Weak momentum prior for {ticker}: μ≈{mu:.3f} ann, σ≈{vol:.3f} "
            f"(horizon {horizon_days}d). Not a trade signal — continuous book still experimental."
            if mu is not None
            else f"Could not form μ for {ticker}."
        ),
        inputs_used=[
            DataRef(
                source=f"history:{ticker}:closes",
                fetched_at=datetime.now(timezone.utc),
            )
        ],
    )


_ = Any
