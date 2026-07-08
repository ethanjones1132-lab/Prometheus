# fincept-sidecar — world markets tracker watchlist (plan §7 Phase 1, Appendix A)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later

from __future__ import annotations

import asyncio
import time
from typing import Any

from . import market_data

# Curated cross-asset watchlist (plan: 132 instruments / 6 classes — expanded incrementally).
TRACKER_BY_CATEGORY: dict[str, list[dict[str, str]]] = {
    "stocks": [
        {"ticker": "SPY", "name": "S&P 500 ETF"},
        {"ticker": "QQQ", "name": "Nasdaq 100 ETF"},
        {"ticker": "AAPL", "name": "Apple"},
        {"ticker": "MSFT", "name": "Microsoft"},
        {"ticker": "NVDA", "name": "NVIDIA"},
    ],
    "etfs": [
        {"ticker": "IWM", "name": "Russell 2000 ETF"},
        {"ticker": "TLT", "name": "20+ Year Treasury ETF"},
        {"ticker": "HYG", "name": "High Yield Corporate Bond ETF"},
    ],
    "crypto": [
        {"ticker": "BTC-USD", "name": "Bitcoin"},
        {"ticker": "ETH-USD", "name": "Ethereum"},
        {"ticker": "SOL-USD", "name": "Solana"},
    ],
    "commodities": [
        {"ticker": "GC=F", "name": "Gold futures"},
        {"ticker": "CL=F", "name": "Crude oil WTI"},
        {"ticker": "SI=F", "name": "Silver futures"},
    ],
    "forex": [
        {"ticker": "EURUSD=X", "name": "EUR/USD"},
        {"ticker": "USDJPY=X", "name": "USD/JPY"},
        {"ticker": "GBPUSD=X", "name": "GBP/USD"},
    ],
    "bonds": [
        {"ticker": "^TNX", "name": "US 10Y yield"},
        {"ticker": "^FVX", "name": "US 5Y yield"},
        {"ticker": "^IRX", "name": "US 13W yield"},
    ],
}

VALID_CATEGORIES = frozenset(TRACKER_BY_CATEGORY.keys())

# Chat / quick snapshot: high-signal macro underlyings for Kalshi index & Fed contracts.
CHAT_SNAPSHOT_TICKERS: list[tuple[str, str, str]] = [
    ("SPY", "S&P 500", "stocks"),
    ("QQQ", "Nasdaq 100", "stocks"),
    ("^VIX", "VIX", "stocks"),
    ("BTC-USD", "Bitcoin", "crypto"),
    ("GC=F", "Gold", "commodities"),
    ("CL=F", "WTI crude", "commodities"),
    ("EURUSD=X", "EUR/USD", "forex"),
    ("^TNX", "US 10Y yield", "bonds"),
]


async def _quote_row(ticker: str, name: str, category: str) -> dict[str, Any]:
    quote = await market_data.get_quote(ticker)
    if quote is None:
        return {
            "ticker": ticker,
            "name": name,
            "category": category,
            "last_price": None,
            "currency": None,
            "error": "no quote",
        }
    return {
        "ticker": ticker,
        "name": name,
        "category": category,
        "last_price": quote.get("last_price"),
        "currency": quote.get("currency"),
        "fetched_at": quote.get("fetched_at"),
        "source": quote.get("source"),
    }


async def get_chat_snapshot() -> dict[str, Any]:
    """Small cross-asset snapshot for LLM context (bounded concurrency)."""
    sem = asyncio.Semaphore(4)

    async def one(ticker: str, name: str, category: str) -> dict[str, Any]:
        async with sem:
            return await _quote_row(ticker, name, category)

    rows = await asyncio.gather(
        *[one(t, n, c) for t, n, c in CHAT_SNAPSHOT_TICKERS],
        return_exceptions=False,
    )
    return {
        "mode": "chat_snapshot",
        "instruments": rows,
        "fetched_at": time.time(),
    }


async def get_tracker(category: str | None = None) -> dict[str, Any]:
    if category is not None and category not in VALID_CATEGORIES:
        return {"error": "invalid_category", "valid": sorted(VALID_CATEGORIES)}

    categories = [category] if category else sorted(TRACKER_BY_CATEGORY.keys())
    sem = asyncio.Semaphore(6)
    instruments: list[dict[str, Any]] = []

    async def fetch_one(cat: str, entry: dict[str, str]) -> None:
        async with sem:
            row = await _quote_row(entry["ticker"], entry["name"], cat)
            instruments.append(row)

    tasks = [
        fetch_one(cat, entry)
        for cat in categories
        for entry in TRACKER_BY_CATEGORY[cat]
    ]
    await asyncio.gather(*tasks)

    instruments.sort(key=lambda r: (r.get("category") or "", r.get("ticker") or ""))
    return {
        "mode": "tracker",
        "category_filter": category,
        "categories": categories,
        "instrument_count": len(instruments),
        "instruments": instruments,
        "fetched_at": time.time(),
    }