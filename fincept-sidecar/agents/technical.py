# Technical agent — distributional P(S_T > K) for price-level contracts (plan §5.1 #2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Data source: yfinance OHLCV via fincept_sidecar.engines.market_data (Apache-2.0
# path). No Fincept-derived code. No hallucinated prices — if history is missing
# or the strike/horizon cannot be inferred, probability=None.

from __future__ import annotations

import math
import re
from datetime import datetime, timezone
from typing import Any

from fincept_sidecar.engines import market_data
from fincept_sidecar.schemas import AgentSignal, DataRef, MarketCategory, MarketOpinionRequest

# Map common Kalshi underlyings / title tokens → yfinance tickers.
# Sources: public Yahoo Finance symbols; Kalshi series naming conventions.
UNDERLYING_TICKERS: dict[str, str] = {
    # Kalshi series prefixes (checked via substring on ticker+title)
    "KXBTCD": "BTC-USD",
    "KXBTC": "BTC-USD",
    "KXETHD": "ETH-USD",
    "KXETH": "ETH-USD",
    "KXSOL": "SOL-USD",
    "KXINX": "^GSPC",
    "INXD": "^GSPC",
    "KXNASDAQ": "^NDX",
    "KXNDX": "^NDX",
    "KXRUT": "^RUT",
    "KXGOLD": "GC=F",
    "KXWTI": "CL=F",
    "KXOIL": "CL=F",
    "KXAAPL": "AAPL",
    "KXTSLA": "TSLA",
    "KXNVDA": "NVDA",
    # Title tokens
    "SPX": "^GSPC",
    "SPY": "SPY",
    "S&P": "^GSPC",
    "S&P 500": "^GSPC",
    "S&P500": "^GSPC",
    "NDX": "^NDX",
    "NASDAQ": "^IXIC",
    "NASDAQ-100": "^NDX",
    "QQQ": "QQQ",
    "RUT": "^RUT",
    "RUSSELL": "^RUT",
    "IWM": "IWM",
    "BTC": "BTC-USD",
    "BITCOIN": "BTC-USD",
    "ETH": "ETH-USD",
    "ETHEREUM": "ETH-USD",
    "SOLANA": "SOL-USD",
    "GOLD": "GC=F",
    "WTI": "CL=F",
    "OIL": "CL=F",
    "AAPL": "AAPL",
    "TSLA": "TSLA",
    "MSFT": "MSFT",
    "NVDA": "NVDA",
    "AMZN": "AMZN",
    "GOOGL": "GOOGL",
    "META": "META",
}


def _norm_cdf(x: float) -> float:
    """Standard normal CDF via math.erf (stdlib only)."""
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def annualized_realized_vol(closes: list[float]) -> float | None:
    """Log-return sample std-dev, annualized with 252 trading days.

    Source: caller-supplied close series (yfinance daily bars).
    Requires ≥10 closes.
    """
    if len(closes) < 10:
        return None
    rets: list[float] = []
    for i in range(1, len(closes)):
        a, b = closes[i - 1], closes[i]
        if a <= 0 or b <= 0:
            continue
        rets.append(math.log(b / a))
    if len(rets) < 9:
        return None
    mean = sum(rets) / len(rets)
    var = sum((r - mean) ** 2 for r in rets) / len(rets)
    daily_sigma = math.sqrt(var)
    return daily_sigma * math.sqrt(252.0)


def binary_call_prob(spot: float, strike: float, sigma: float, tau_years: float, mu: float = 0.0) -> float:
    """Risk-neutral-ish P(S_T > K) under lognormal dynamics (plan §5.1).

    Φ( (ln(S/K) + (μ − σ²/2)·τ) / (σ·√τ) ) with μ default 0 (conservative).
    """
    if spot <= 0 or strike <= 0 or sigma <= 0 or tau_years <= 0:
        raise ValueError("invalid binary_call_prob inputs")
    numer = math.log(spot / strike) + (mu - 0.5 * sigma * sigma) * tau_years
    denom = sigma * math.sqrt(tau_years)
    return _norm_cdf(numer / denom)


def binary_put_prob(spot: float, strike: float, sigma: float, tau_years: float, mu: float = 0.0) -> float:
    """P(S_T < K) under the same lognormal model as binary_call_prob."""
    return 1.0 - binary_call_prob(spot, strike, sigma, tau_years, mu=mu)


def binary_bracket_prob(
    spot: float,
    floor: float,
    cap: float,
    sigma: float,
    tau_years: float,
    mu: float = 0.0,
) -> float:
    """P(floor < S_T < cap) = P(S>floor) − P(S>cap) under lognormal dynamics.

    Kalshi B-leg hourlies are **range** contracts (between floor and cap), not
    cumulative above the B-number. Pricing them as P(S>K) massively overstates
    YES on every lower bin when spot sits above that bin.
    """
    if cap <= floor:
        raise ValueError("cap must exceed floor")
    # Survival difference; clamp tiny negatives from float noise.
    return max(0.0, binary_call_prob(spot, floor, sigma, tau_years, mu=mu) - binary_call_prob(spot, cap, sigma, tau_years, mu=mu))


def clamp_prob(p: float) -> float:
    return max(0.01, min(0.99, p))


# Contract geometry for Kalshi price-level markets.
# B-legs = bracket/range; T-legs = one-sided threshold (above OR below).
STYLE_ABOVE = "above"
STYLE_BELOW = "below"
STYLE_BRACKET = "bracket"


def _fnum(val: Any) -> float | None:
    if isinstance(val, (int, float)) and float(val) > 0:
        return float(val)
    if isinstance(val, str):
        try:
            f = float(val.replace(",", "").replace("$", "").strip())
            return f if f > 0 else None
        except ValueError:
            return None
    return None


def _bracket_bounds_from_center(center: float) -> tuple[float, float]:
    """Best-effort floor/cap when API bounds are missing.

    Observed Kalshi conventions (2026-07):
      - crypto / high NDX B-centers (≥10_000): $100-wide bins, center = mid
      - SPX-style (~1_000–9_999): $25-wide bins
      - else: $1-wide
    """
    if center >= 10_000:
        half = 50.0
    elif center >= 1_000:
        half = 12.5
    else:
        half = 0.5
    floor = center - half
    cap = center + half - 0.01
    return floor, cap


def infer_contract_spec(req: MarketOpinionRequest) -> dict[str, Any] | None:
    """Infer {style, floor, cap, label} for price-level YES resolution.

    Priority:
      1. context.floor_strike / context.cap_strike (from Kalshi API — preferred)
      2. context.contract_style + strike
      3. ticker suffix -B# (bracket) / -T# (threshold; direction from rules/title)
      4. free-text between/above/below in title+rules
    """
    ctx = req.context or {}
    floor = _fnum(ctx.get("floor_strike"))
    cap = _fnum(ctx.get("cap_strike"))
    style_ctx = ctx.get("contract_style")
    if isinstance(style_ctx, str):
        style_ctx = style_ctx.strip().lower()
    else:
        style_ctx = None

    # Explicit API bounds win.
    if floor is not None and cap is not None and cap > floor:
        return {
            "style": STYLE_BRACKET,
            "floor": floor,
            "cap": cap,
            "label": f"bracket[{floor:g},{cap:g}]",
        }
    if floor is not None and cap is None:
        return {
            "style": STYLE_ABOVE,
            "floor": floor,
            "cap": None,
            "label": f"above {floor:g}",
        }
    if cap is not None and floor is None:
        return {
            "style": STYLE_BELOW,
            "floor": None,
            "cap": cap,
            "label": f"below {cap:g}",
        }

    if style_ctx in (STYLE_ABOVE, STYLE_BELOW, STYLE_BRACKET):
        k = _fnum(ctx.get("strike") or ctx.get("threshold") or ctx.get("level") or ctx.get("K"))
        if style_ctx == STYLE_ABOVE and k is not None:
            return {"style": STYLE_ABOVE, "floor": k, "cap": None, "label": f"above {k:g}"}
        if style_ctx == STYLE_BELOW and k is not None:
            return {"style": STYLE_BELOW, "floor": None, "cap": k, "label": f"below {k:g}"}
        if style_ctx == STYLE_BRACKET and k is not None:
            f, c = _bracket_bounds_from_center(k)
            return {"style": STYLE_BRACKET, "floor": f, "cap": c, "label": f"bracket~{k:g}"}

    text = f"{req.title} {req.resolution_rules} {req.market_ticker}"
    text_l = text.lower()

    # Ticker geometry: -B73250 (range) vs -T73299.99 (threshold).
    m_b = re.search(r"(?i)[-_]B(\d+(?:\.\d+)?)(?:\b|$)", req.market_ticker)
    m_t = re.search(r"(?i)[-_]T(\d+(?:\.\d+)?)(?:\b|$)", req.market_ticker)
    if m_b:
        center = float(m_b.group(1))
        f, c = _bracket_bounds_from_center(center)
        return {
            "style": STYLE_BRACKET,
            "floor": f,
            "cap": c,
            "label": f"B{center:g}→[{f:g},{c:g}]",
        }
    if m_t:
        k = float(m_t.group(1))
        # Direction from rules/title; default above for "T" when ambiguous is wrong
        # for low tails — require lexical cue, else treat large-K as above / small as below
        # only when text is silent.
        if re.search(r"\b(below|under|less than|or below)\b", text_l):
            return {"style": STYLE_BELOW, "floor": None, "cap": k, "label": f"T{k:g} below"}
        if re.search(r"\b(above|over|greater than|or above)\b", text_l):
            return {"style": STYLE_ABOVE, "floor": k, "cap": None, "label": f"T{k:g} above"}
        # No cue: high absolute levels are usually the upper tail ("or above").
        return {"style": STYLE_ABOVE, "floor": k, "cap": None, "label": f"T{k:g} above(default)"}

    m_between = re.search(
        r"between\s+\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.\d+)?)\s+and\s+\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.\d+)?)",
        text_l,
    )
    if m_between:
        a = float(m_between.group(1).replace(",", ""))
        b = float(m_between.group(2).replace(",", ""))
        lo, hi = (a, b) if a < b else (b, a)
        return {"style": STYLE_BRACKET, "floor": lo, "cap": hi, "label": f"between {lo:g}-{hi:g}"}

    m_above = re.search(
        r"(?:above|over|greater than|exceed(?:s|ing)?)\s+\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.\d+)?)",
        text_l,
    )
    if m_above:
        k = float(m_above.group(1).replace(",", ""))
        return {"style": STYLE_ABOVE, "floor": k, "cap": None, "label": f"above {k:g}"}

    m_below = re.search(
        r"(?:below|under|less than)\s+\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.\d+)?)",
        text_l,
    )
    if m_below:
        k = float(m_below.group(1).replace(",", ""))
        return {"style": STYLE_BELOW, "floor": None, "cap": k, "label": f"below {k:g}"}

    # Legacy single-strike fallback (ambiguous geometry → refuse rather than
    # silently treat as call). Callers that only have a strike must set style.
    return None


def price_contract_prob(
    spot: float,
    spec: dict[str, Any],
    sigma: float,
    tau_years: float,
    mu: float = 0.0,
) -> float:
    """P(YES) for a Kalshi-style price-level contract under lognormal S_T."""
    style = spec["style"]
    if style == STYLE_ABOVE:
        return binary_call_prob(spot, float(spec["floor"]), sigma, tau_years, mu=mu)
    if style == STYLE_BELOW:
        return binary_put_prob(spot, float(spec["cap"]), sigma, tau_years, mu=mu)
    if style == STYLE_BRACKET:
        return binary_bracket_prob(
            spot, float(spec["floor"]), float(spec["cap"]), sigma, tau_years, mu=mu
        )
    raise ValueError(f"unknown contract style {style}")


def infer_underlying_ticker(req: MarketOpinionRequest) -> str | None:
    """Best-effort underlying from context or title/ticker tokens."""
    ctx = req.context or {}
    for key in ("underlying_ticker", "yf_ticker", "asset_ticker"):
        val = ctx.get(key)
        if isinstance(val, str) and val.strip():
            return val.strip()

    hay = f"{req.market_ticker} {req.title} {req.resolution_rules}".upper()
    # Prefer longer keys first (S&P 500 before S&P).
    # Short equity/crypto tokens use word boundaries so "something" ≠ ETH.
    for token in sorted(UNDERLYING_TICKERS.keys(), key=len, reverse=True):
        t = token.upper()
        if len(t) <= 4 and t.isalpha():
            if re.search(rf"\b{re.escape(t)}\b", hay):
                return UNDERLYING_TICKERS[token]
        elif t in hay:
            return UNDERLYING_TICKERS[token]
    return None


def infer_strike(req: MarketOpinionRequest) -> float | None:
    """Legacy single-level helper.

    Prefer :func:`infer_contract_spec` — B-legs are brackets, not a single K.
    Returns a representative level (bracket mid, or the one-sided threshold)
    for display / moneyness only.
    """
    ctx = req.context or {}
    for key in ("strike", "threshold", "level", "K"):
        val = _fnum(ctx.get(key))
        if val is not None:
            return val

    spec = infer_contract_spec(req)
    if spec is None:
        return None
    if spec["style"] == STYLE_ABOVE:
        return float(spec["floor"]) if spec.get("floor") is not None else None
    if spec["style"] == STYLE_BELOW:
        return float(spec["cap"]) if spec.get("cap") is not None else None
    # bracket mid
    return 0.5 * (float(spec["floor"]) + float(spec["cap"]))


def years_to_close(close_time: datetime, horizon_days: float | None = None) -> float | None:
    """Prefer explicit horizon_days from Rust; else derive from close_time."""
    if horizon_days is not None and horizon_days > 0:
        return horizon_days / 365.25
    now = datetime.now(timezone.utc)
    if close_time.tzinfo is None:
        close_time = close_time.replace(tzinfo=timezone.utc)
    seconds = (close_time - now).total_seconds()
    if seconds <= 0:
        return None
    return seconds / (365.25 * 24 * 3600)


def _horizon_days_from_context(ctx: dict[str, Any]) -> float | None:
    val = ctx.get("horizon_days")
    if isinstance(val, (int, float)) and val > 0:
        return float(val)
    if isinstance(val, str):
        try:
            f = float(val)
            return f if f > 0 else None
        except ValueError:
            return None
    return None


def _momentum_mu(closes: list[float]) -> float:
    """Tiny annualized drift from trailing 20-day log return; bounded ±20%."""
    if len(closes) < 21:
        return 0.0
    a, b = closes[-21], closes[-1]
    if a <= 0 or b <= 0:
        return 0.0
    # 20 trading days ≈ 20/252 year
    r = math.log(b / a)
    mu = r * (252.0 / 20.0)
    return max(-0.20, min(0.20, mu))


async def estimate(req: MarketOpinionRequest) -> AgentSignal:
    """Return P(YES) for index/asset price-level contracts, else no opinion."""
    caveats: list[str] = []
    # Not routed for pure political/other unless context forces an underlying.
    if req.category not in (
        MarketCategory.INDEX_PRICE_LEVEL,
        MarketCategory.COMPANY_EVENT,
        MarketCategory.OTHER,
    ):
        # Still try if context explicitly supplies underlying+strike (Rust can force).
        if not (req.context or {}).get("underlying_ticker"):
            return AgentSignal(
                agent="technical",
                probability=None,
                confidence=0.0,
                rationale=(
                    "Technical agent only opines on price-level style contracts "
                    f"(category={req.category.value}); no underlying forced in context."
                ),
                inputs_used=[],
                caveats=["category_out_of_scope"],
            )

    yf_ticker = infer_underlying_ticker(req)
    spec = infer_contract_spec(req)
    h_days = _horizon_days_from_context(req.context or {})
    tau = years_to_close(req.close_time, horizon_days=h_days)

    if yf_ticker is None or spec is None or tau is None:
        missing = []
        if yf_ticker is None:
            missing.append("underlying")
        if spec is None:
            missing.append("contract_geometry")
        if tau is None:
            missing.append("horizon")
        return AgentSignal(
            agent="technical",
            probability=None,
            confidence=0.0,
            rationale=(
                "Cannot form a distributional price forecast: missing "
                + ", ".join(missing)
                + ". Refusing to invent a probability. "
                "Note: Kalshi B-legs are range/bracket contracts — a bare strike "
                "without style/bounds is not enough."
            ),
            inputs_used=[],
            caveats=[f"missing:{m}" for m in missing],
        )

    history = await market_data.get_history(yf_ticker, period="3mo", interval="1d")
    quote = await market_data.get_quote(yf_ticker)
    if history is None or quote is None:
        return AgentSignal(
            agent="technical",
            probability=None,
            confidence=0.0,
            rationale=f"No yfinance data for {yf_ticker}; no opinion.",
            inputs_used=[],
            caveats=["yfinance_unavailable"],
        )

    bars: list[dict[str, Any]] = history.get("bars") or []
    closes = [float(b["close"]) for b in bars if b.get("close") is not None]
    spot = float(quote["last_price"])
    sigma = annualized_realized_vol(closes)
    if sigma is None or sigma <= 0:
        return AgentSignal(
            agent="technical",
            probability=None,
            confidence=0.0,
            rationale=f"Insufficient close history for realized vol on {yf_ticker}.",
            inputs_used=[
                DataRef(source=f"yfinance:{yf_ticker}:history", fetched_at=datetime.now(timezone.utc)),
            ],
            caveats=["insufficient_history"],
        )

    mu = _momentum_mu(closes)
    try:
        raw_p = price_contract_prob(spot, spec, sigma, tau, mu=mu)
    except ValueError as e:
        return AgentSignal(
            agent="technical",
            probability=None,
            confidence=0.0,
            rationale=f"Invalid contract geometry for {yf_ticker}: {e}",
            inputs_used=[],
            caveats=["invalid_contract_geometry"],
        )
    p = clamp_prob(raw_p)

    # Representative level for moneyness / confidence.
    if spec["style"] == STYLE_BRACKET:
        strike_ref = 0.5 * (float(spec["floor"]) + float(spec["cap"]))
        caveats.append("bracket_range_contract")
    elif spec["style"] == STYLE_BELOW:
        strike_ref = float(spec["cap"])
        caveats.append("below_threshold_contract")
    else:
        strike_ref = float(spec["floor"])
        caveats.append("above_threshold_contract")

    # Confidence: higher when more bars, moderate moneyness, and longer but not multi-year horizon.
    n_bars = len(closes)
    moneyness = abs(math.log(spot / strike_ref)) if strike_ref > 0 else 1.0
    conf = 0.35
    conf += min(0.25, n_bars / 200.0)
    if moneyness < 0.08:
        conf += 0.15  # near ATM — vol estimate matters, still informative
    elif moneyness > 0.35:
        conf -= 0.10
        caveats.append("deep_otm_or_itm")
    if tau < 1 / 365:  # < 1 day
        conf -= 0.15
        caveats.append("very_short_horizon")
        # Daily realized vol is a poor short-horizon estimator; haircut confidence harder.
        conf -= 0.10
        caveats.append("daily_vol_on_intraday_horizon")
    if tau > 1.0:
        conf -= 0.10
        caveats.append("horizon_gt_1y")
    # Bracket bins are narrow — density is sensitive to vol; keep conf modest.
    if spec["style"] == STYLE_BRACKET:
        conf -= 0.05
    conf = max(0.05, min(0.85, conf))

    fetched_hist = datetime.fromtimestamp(float(history.get("fetched_at", 0)), tz=timezone.utc)
    fetched_quote = datetime.fromtimestamp(float(quote.get("fetched_at", 0)), tz=timezone.utc)

    return AgentSignal(
        agent="technical",
        probability=p,
        confidence=conf,
        rationale=(
            f"Lognormal P(YES|{spec['label']}) for {yf_ticker}: spot={spot:.4g}, "
            f"σ_realized={sigma:.3f} (daily closes), τ={tau*365.25:.1f}d, μ_mom={mu:.3f}. "
            f"Raw Φ-prob={raw_p:.4f} → clamped {p:.4f}. "
            f"Style={spec['style']} (B=bracket range, T=one-sided). "
            "Source: yfinance history+quote."
        ),
        inputs_used=[
            DataRef(source=f"yfinance:{yf_ticker}:quote", fetched_at=fetched_quote),
            DataRef(source=f"yfinance:{yf_ticker}:history:3mo:1d", fetched_at=fetched_hist),
        ],
        caveats=caveats,
    )
