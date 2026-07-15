# Macro agent — public FRED series for economic contracts (Sprint 4)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Honesty rules:
#   - probability=None when series is unmapped or FRED key/data missing
#   - Never invent CPI/Fed prints
#   - Opines only with a numeric threshold + direction in title/rules
#   - Optional FRED_API_KEY env (free at https://fred.stlouisfed.org/docs/api/api_key.html)

from __future__ import annotations

import math
import os
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import urlopen

from fincept_sidecar.schemas import AgentSignal, DataRef, MarketCategory, MarketOpinionRequest

# Kalshi series / title tokens → FRED series id + transform.
# transform: "level" | "yoy" (year-over-year % change) | "mom" (month-over-month)
SERIES_MAP: list[tuple[tuple[str, ...], str, str, str]] = [
    # (match tokens uppercased, fred_id, transform, human_label)
    (("KXCPI", "CPI YOY", "CPI Y/Y", "INFLATION RATE", "HEADLINE CPI"), "CPIAUCSL", "yoy", "CPI (all urban)"),
    (("CORE CPI", "CPILFE", "CORE INFLATION"), "CPILFESL", "yoy", "Core CPI"),
    (("KXUNRATE", "UNEMPLOYMENT", "JOBLESS RATE", "U-3"), "UNRATE", "level", "Unemployment rate"),
    (("KXPAYROLL", "NONFARM", "PAYROLLS", "NFP", "JOBS REPORT"), "PAYEMS", "mom", "Nonfarm payrolls"),
    (("KXGDP", "REAL GDP", "GDP GROWTH"), "A191RL1Q225SBEA", "level", "Real GDP growth (SAAR)"),
    (("FED FUNDS", "FEDERAL FUNDS", "POLICY RATE", "KXFEDRATE", "FFR"), "FEDFUNDS", "level", "Fed funds rate"),
    (("KXRATE", "INTEREST RATE", "FOMC", "FED CUT", "FED HIKE", "RATE CUT", "RATE HIKE"), "DFF", "level", "Effective fed funds"),
    (("PCE", "KXPCE"), "PCEPI", "yoy", "PCE price index"),
    (("INITIAL CLAIMS", "JOBLESS CLAIMS"), "ICSA", "level", "Initial jobless claims"),
]


@dataclass(frozen=True)
class MacroMapping:
    fred_id: str
    transform: str
    label: str
    matched_token: str


@dataclass(frozen=True)
class ThresholdSpec:
    value: float
    direction: str  # "above" | "below" | "at_least" | "at_most"


def infer_macro_mapping(req: MarketOpinionRequest) -> MacroMapping | None:
    """Best-effort series map from ticker/title/rules. None = unmapped."""
    ctx = req.context or {}
    forced = ctx.get("fred_series_id") or ctx.get("macro_series")
    if isinstance(forced, str) and forced.strip():
        transform = str(ctx.get("fred_transform") or "level").lower()
        if transform not in ("level", "yoy", "mom"):
            transform = "level"
        return MacroMapping(
            fred_id=forced.strip().upper(),
            transform=transform,
            label=forced.strip().upper(),
            matched_token="context",
        )

    hay = f"{req.market_ticker} {req.title} {req.resolution_rules}".upper()
    best: MacroMapping | None = None
    best_len = 0
    for tokens, fred_id, transform, label in SERIES_MAP:
        for tok in tokens:
            if tok in hay and len(tok) > best_len:
                best_len = len(tok)
                best = MacroMapping(fred_id=fred_id, transform=transform, label=label, matched_token=tok)
    return best


def infer_threshold(text: str) -> ThresholdSpec | None:
    """Parse level thresholds from contract language."""
    patterns = [
        (r"(?i)(?:above|over|greater than|exceed(?:s|ing)?|at least|≥|>=)\s*(-?\d+(?:\.\d+)?)\s*%?", "above"),
        (r"(?i)(?:below|under|less than|at most|≤|<=)\s*(-?\d+(?:\.\d+)?)\s*%?", "below"),
        (r"(?i)(?:between)\s*(-?\d+(?:\.\d+)?)", "above"),  # floor only; weak
    ]
    for pat, direction in patterns:
        m = re.search(pat, text)
        if m:
            try:
                v = float(m.group(1))
                if abs(v) > 0:
                    return ThresholdSpec(value=v, direction=direction)
            except ValueError:
                continue
    # Bare percent after "to" / "by" e.g. "cut by 25 bps" → skip (not a level)
    m = re.search(r"(?i)(-?\d+(?:\.\d+)?)\s*%", text)
    if m:
        try:
            v = float(m.group(1))
            # Prefer "above" default for "will X be 3%?" style
            if "below" in text.lower() or "under" in text.lower():
                return ThresholdSpec(value=v, direction="below")
            return ThresholdSpec(value=v, direction="above")
        except ValueError:
            pass
    return None


def yoy_pct(values: list[float], lag: int = 12) -> float | None:
    if len(values) <= lag:
        return None
    a, b = values[-(lag + 1)], values[-1]
    if a == 0:
        return None
    return 100.0 * (b / a - 1.0)


def mom_change(values: list[float]) -> float | None:
    if len(values) < 2:
        return None
    return values[-1] - values[-2]


def realized_level(values: list[float], transform: str) -> float | None:
    if not values:
        return None
    if transform == "yoy":
        return yoy_pct(values, 12)
    if transform == "mom":
        # PAYEMS is thousands of persons — mom change in thousands is common on Kalshi
        return mom_change(values)
    return values[-1]


def binary_from_level(
    level: float,
    threshold: ThresholdSpec,
    sigma: float,
) -> float:
    """Rough Φ((level−K)/σ) for above; flip for below. Caps [0.01, 0.99]."""
    sig = max(sigma, 1e-6)
    z = (level - threshold.value) / sig
    # standard normal CDF via erf
    p_above = 0.5 * (1.0 + math.erf(z / math.sqrt(2.0)))
    if threshold.direction in ("below", "at_most"):
        p = 1.0 - p_above
    else:
        p = p_above
    return max(0.01, min(0.99, p))


def estimate_sigma(values: list[float], transform: str) -> float:
    """Dispersion of the transformed series for a conservative σ."""
    series: list[float] = []
    if transform == "yoy":
        for i in range(12, len(values)):
            a, b = values[i - 12], values[i]
            if a != 0:
                series.append(100.0 * (b / a - 1.0))
    elif transform == "mom":
        for i in range(1, len(values)):
            series.append(values[i] - values[i - 1])
    else:
        series = list(values[-24:]) if len(values) >= 2 else list(values)

    if len(series) < 3:
        # Defaults by transform units
        return {"yoy": 0.4, "mom": 50.0, "level": 0.25}.get(transform, 0.5)

    mean = sum(series) / len(series)
    var = sum((x - mean) ** 2 for x in series) / len(series)
    return max(math.sqrt(var), 1e-3)


def fetch_fred_observations(series_id: str, api_key: str, limit: int = 36) -> list[float] | None:
    """Pull latest FRED observations (ascending). Returns None on any failure."""
    params = urlencode(
        {
            "series_id": series_id,
            "api_key": api_key,
            "file_type": "json",
            "sort_order": "desc",
            "limit": str(limit),
        }
    )
    url = f"https://api.stlouisfed.org/fred/series/observations?{params}"
    try:
        with urlopen(url, timeout=8) as resp:
            import json

            body = json.loads(resp.read().decode("utf-8"))
    except (HTTPError, URLError, TimeoutError, ValueError, OSError):
        return None

    obs = body.get("observations") or []
    values: list[float] = []
    for row in reversed(obs):  # chronological
        v = row.get("value")
        if v is None or v == ".":
            continue
        try:
            values.append(float(v))
        except (TypeError, ValueError):
            continue
    return values if len(values) >= 2 else None


def resolve_fred_api_key(ctx: dict[str, Any] | None = None) -> str | None:
    if ctx:
        k = ctx.get("fred_api_key")
        if isinstance(k, str) and k.strip():
            return k.strip()
    env = os.environ.get("FRED_API_KEY") or os.environ.get("fred_api_key")
    if env and env.strip():
        return env.strip()
    return None


def estimate_sync(
    req: MarketOpinionRequest,
    *,
    observations: list[float] | None = None,
) -> AgentSignal:
    """
    Pure/sync path. Pass observations= to unit-test without network.
    When observations is None, attempts FRED if key present.
    """
    # Category gate: economic or forced series
    cat_ok = req.category in (MarketCategory.ECONOMIC, MarketCategory.OTHER)
    mapping = infer_macro_mapping(req)
    if mapping is None:
        return AgentSignal(
            agent="macro",
            probability=None,
            confidence=0.0,
            rationale=(
                "No series mapping for this contract (CPI/Fed/payrolls/GDP/unemployment "
                "patterns not recognized). Refusing to invent a macro probability."
            ),
            inputs_used=[],
            caveats=["no_series_mapping"],
        )
    if not cat_ok and not (req.context or {}).get("fred_series_id"):
        # Still allow when mapping from ticker prefix (KXCPI etc.)
        if not mapping.matched_token.startswith("KX") and mapping.matched_token != "context":
            return AgentSignal(
                agent="macro",
                probability=None,
                confidence=0.0,
                rationale=(
                    f"Macro agent defers on category={req.category.value}; "
                    f"mapped {mapping.label} but category is not economic."
                ),
                inputs_used=[],
                caveats=["category_out_of_scope"],
            )

    thr = infer_threshold(f"{req.title} {req.resolution_rules}")
    if thr is None:
        return AgentSignal(
            agent="macro",
            probability=None,
            confidence=0.0,
            rationale=(
                f"Mapped to FRED {mapping.fred_id} ({mapping.label}) via '{mapping.matched_token}', "
                "but no numeric threshold/direction in title/rules — cannot form P(YES)."
            ),
            inputs_used=[],
            caveats=["missing:threshold"],
        )

    values = observations
    key_used = False
    if values is None:
        key = resolve_fred_api_key(req.context if isinstance(req.context, dict) else None)
        if not key:
            return AgentSignal(
                agent="macro",
                probability=None,
                confidence=0.0,
                rationale=(
                    f"Mapped to FRED {mapping.fred_id} ({mapping.label}), threshold "
                    f"{thr.direction} {thr.value:g}, but FRED_API_KEY is not set. "
                    "Set env FRED_API_KEY (free) for live series; no fabricated print."
                ),
                inputs_used=[],
                caveats=["missing:fred_api_key"],
            )
        values = fetch_fred_observations(mapping.fred_id, key)
        key_used = True
        if values is None:
            return AgentSignal(
                agent="macro",
                probability=None,
                confidence=0.0,
                rationale=(
                    f"FRED fetch failed or empty for {mapping.fred_id}. No opinion."
                ),
                inputs_used=[
                    DataRef(
                        source=f"fred:{mapping.fred_id}",
                        fetched_at=datetime.now(timezone.utc),
                    )
                ],
                caveats=["fred_fetch_failed"],
            )

    level = realized_level(values, mapping.transform)
    if level is None:
        return AgentSignal(
            agent="macro",
            probability=None,
            confidence=0.0,
            rationale=f"Insufficient observations to compute {mapping.transform} for {mapping.fred_id}.",
            inputs_used=[],
            caveats=["insufficient_history"],
        )

    sigma = estimate_sigma(values, mapping.transform)
    p = binary_from_level(level, thr, sigma)
    conf = 0.25
    conf += min(0.2, len(values) / 100.0)
    if abs(level - thr.value) / max(sigma, 1e-6) > 2.0:
        conf += 0.1  # deep ITM/OTM — more confident
    conf = max(0.08, min(0.55, conf))

    return AgentSignal(
        agent="macro",
        probability=p,
        confidence=conf,
        rationale=(
            f"FRED {mapping.fred_id} ({mapping.label}) {mapping.transform}={level:.3g}; "
            f"threshold {thr.direction} {thr.value:g}; σ≈{sigma:.3g} → P(YES)={p:.3f}. "
            f"Heuristic distributional model{' with live FRED' if key_used else ' (injected series)'} "
            "— not a BLS print forecast."
        ),
        inputs_used=[
            DataRef(
                source=f"fred:{mapping.fred_id}:{mapping.transform}",
                fetched_at=datetime.now(timezone.utc),
            )
        ],
        caveats=["heuristic_only", "not_official_release"],
    )


async def estimate(req: MarketOpinionRequest) -> AgentSignal:
    return estimate_sync(req)
