# fincept-sidecar — market data engine (plan §7 Phase 1, §10.3)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Original code over yfinance (Apache-2.0), on the permissive path (§3 Rule 5b).
#
# Caching per plan §10.3: quotes 60 s, OHLCV history 15 min, in-memory,
# single-flight (concurrent requests for the same key share one fetch).
# Every cached value carries its fetch timestamp so agent responses can
# propagate staleness into AgentSignal.inputs_used (§4.5).

from __future__ import annotations

import asyncio
import time
from typing import Any, Awaitable, Callable

from fastapi import HTTPException

QUOTE_TTL_SECONDS = 60.0
HISTORY_TTL_SECONDS = 15 * 60.0

_cache: dict[str, tuple[float, Any]] = {}
_locks: dict[str, asyncio.Lock] = {}
_locks_guard = asyncio.Lock()


async def _single_flight(key: str, ttl: float, fetch: Callable[[], Awaitable[Any]]) -> Any:
    now = time.monotonic()
    hit = _cache.get(key)
    if hit is not None and now - hit[0] < ttl:
        return hit[1]

    async with _locks_guard:
        lock = _locks.setdefault(key, asyncio.Lock())

    async with lock:
        # Re-check under the lock: a concurrent caller may have filled it.
        hit = _cache.get(key)
        if hit is not None and time.monotonic() - hit[0] < ttl:
            return hit[1]
        value = await fetch()
        _cache[key] = (time.monotonic(), value)
        return value


def _require_yfinance():
    try:
        import yfinance  # noqa: PLC0415 — optional dependency, imported lazily

        return yfinance
    except ImportError:
        raise HTTPException(
            status_code=503,
            detail="market data unavailable: install with `pip install fincept-sidecar[market]`",
        )


async def get_quote(ticker: str) -> dict | None:
    yf = _require_yfinance()

    async def fetch() -> dict | None:
        def sync() -> dict | None:
            t = yf.Ticker(ticker)
            info = t.fast_info
            last = getattr(info, "last_price", None)
            if last is None:
                return None
            return {
                "ticker": ticker,
                "last_price": float(last),
                "currency": getattr(info, "currency", None),
                "fetched_at": time.time(),
                "source": "yfinance",
            }

        return await asyncio.to_thread(sync)

    return await _single_flight(f"quote:{ticker}", QUOTE_TTL_SECONDS, fetch)


async def get_history(ticker: str, period: str = "1mo", interval: str = "1d") -> dict | None:
    yf = _require_yfinance()

    async def fetch() -> dict | None:
        def sync() -> dict | None:
            df = yf.Ticker(ticker).history(period=period, interval=interval)
            if df is None or df.empty:
                return None
            return {
                "ticker": ticker,
                "period": period,
                "interval": interval,
                "bars": [
                    {
                        "ts": ts.isoformat(),
                        "open": float(row["Open"]),
                        "high": float(row["High"]),
                        "low": float(row["Low"]),
                        "close": float(row["Close"]),
                        "volume": float(row["Volume"]),
                    }
                    for ts, row in df.iterrows()
                ],
                "fetched_at": time.time(),
                "source": "yfinance",
            }

        return await asyncio.to_thread(sync)

    return await _single_flight(f"history:{ticker}:{period}:{interval}", HISTORY_TTL_SECONDS, fetch)
