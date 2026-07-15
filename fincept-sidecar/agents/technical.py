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


def clamp_prob(p: float) -> float:
    return max(0.01, min(0.99, p))


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
    """Parse a strike level from context or free text.

    Accepts context.strike / context.threshold, else patterns like
    'above 5500', 'over $300', 'exceed 3.0' (last is often CPI — rejected if < 50
    for equity-like underlyings via caller confidence).
    """
    ctx = req.context or {}
    for key in ("strike", "threshold", "level", "K"):
        val = ctx.get(key)
        if isinstance(val, (int, float)) and val > 0:
            return float(val)
        if isinstance(val, str):
            try:
                f = float(val.replace(",", "").replace("$", ""))
                if f > 0:
                    return f
            except ValueError:
                pass

    # Kalshi barrier tickers: KXBTCD-26JUL15-B100000 / -T95000
    m_tick = re.search(r"(?i)[-_](?:B|T|C)(\d{3,7})(?:\b|$)", req.market_ticker)
    if m_tick:
        try:
            return float(m_tick.group(1))
        except ValueError:
            pass

    text = f"{req.title} {req.resolution_rules}"
    patterns = [
        r"(?:above|over|exceed(?:s|ing)?|greater than|below|under|less than|>\s*|<\s*)\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]+)?)",
        r"(?:close\s+(?:at|above|below)|settle\s+(?:above|below))\s+\$?\s*([0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]+)?)",
        r"\$\s*([0-9]{2,6}(?:\.[0-9]+)?)",
    ]
    for pat in patterns:
        m = re.search(pat, text, re.IGNORECASE)
        if m:
            try:
                return float(m.group(1).replace(",", ""))
            except ValueError:
                continue
    return None


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
    strike = infer_strike(req)
    h_days = _horizon_days_from_context(req.context or {})
    tau = years_to_close(req.close_time, horizon_days=h_days)

    if yf_ticker is None or strike is None or tau is None:
        missing = []
        if yf_ticker is None:
            missing.append("underlying")
        if strike is None:
            missing.append("strike")
        if tau is None:
            missing.append("horizon")
        return AgentSignal(
            agent="technical",
            probability=None,
            confidence=0.0,
            rationale=(
                "Cannot form a distributional price forecast: missing "
                + ", ".join(missing)
                + ". Refusing to invent a probability."
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
    raw_p = binary_call_prob(spot, strike, sigma, tau, mu=mu)
    p = clamp_prob(raw_p)

    # Confidence: higher when more bars, moderate moneyness, and longer but not multi-year horizon.
    n_bars = len(closes)
    moneyness = abs(math.log(spot / strike))
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
    if tau > 1.0:
        conf -= 0.10
        caveats.append("horizon_gt_1y")
    conf = max(0.05, min(0.85, conf))

    fetched_hist = datetime.fromtimestamp(float(history.get("fetched_at", 0)), tz=timezone.utc)
    fetched_quote = datetime.fromtimestamp(float(quote.get("fetched_at", 0)), tz=timezone.utc)

    return AgentSignal(
        agent="technical",
        probability=p,
        confidence=conf,
        rationale=(
            f"Lognormal binary P(S_T>{strike:g}) for {yf_ticker}: spot={spot:.4g}, "
            f"σ_realized={sigma:.3f} (daily closes), τ={tau*365.25:.1f}d, μ_mom={mu:.3f}. "
            f"Raw Φ-prob={raw_p:.4f} → clamped {p:.4f}. Source: yfinance history+quote."
        ),
        inputs_used=[
            DataRef(source=f"yfinance:{yf_ticker}:quote", fetched_at=fetched_quote),
            DataRef(source=f"yfinance:{yf_ticker}:history:3mo:1d", fetched_at=fetched_hist),
        ],
        caveats=caveats,
    )
