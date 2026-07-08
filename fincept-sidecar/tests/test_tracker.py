# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for world-markets tracker (no live yfinance)."""

from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, patch

from fincept_sidecar.engines import tracker


def test_get_tracker_all_categories_mocked():
    fake_quote = {"last_price": 100.0, "currency": "USD", "fetched_at": 1.0, "source": "yfinance"}

    async def run():
        with patch(
            "fincept_sidecar.engines.tracker.market_data.get_quote",
            new_callable=AsyncMock,
            return_value=fake_quote,
        ):
            return await tracker.get_tracker(None)

    payload = asyncio.run(run())

    assert payload["mode"] == "tracker"
    assert payload["instrument_count"] > 0
    assert all(r.get("last_price") == 100.0 for r in payload["instruments"])


def test_get_chat_snapshot_shape():
    async def run():
        with patch(
            "fincept_sidecar.engines.tracker.market_data.get_quote",
            new_callable=AsyncMock,
            return_value=None,
        ):
            return await tracker.get_chat_snapshot()

    payload = asyncio.run(run())

    assert payload["mode"] == "chat_snapshot"
    assert len(payload["instruments"]) == len(tracker.CHAT_SNAPSHOT_TICKERS)
    assert all(r.get("error") == "no quote" for r in payload["instruments"])


def test_valid_categories_cover_watchlist():
    assert tracker.VALID_CATEGORIES == frozenset(tracker.TRACKER_BY_CATEGORY.keys())