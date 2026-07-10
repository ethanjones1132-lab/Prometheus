# SPDX-License-Identifier: AGPL-3.0-or-later
"""Live smoke: technical (yfinance) + contract_tape on a synthetic S&P request."""

from __future__ import annotations

import asyncio
from datetime import datetime, timedelta, timezone

from agents.orchestrator import collect_market_opinion
from fincept_sidecar.schemas import MarketCategory, MarketOpinionRequest


async def main() -> None:
    req = MarketOpinionRequest(
        market_ticker="KXINX-TEST-5500",
        title="Will S&P 500 close above 5500?",
        resolution_rules="Resolves YES if S&P 500 closes above 5500 before expiry.",
        close_time=datetime.now(timezone.utc) + timedelta(days=30),
        category=MarketCategory.INDEX_PRICE_LEVEL,
        yes_bid=0.55,
        yes_ask=0.57,
        context={
            "underlying_ticker": "SPY",
            "strike": 550.0,
            "contract_mids": [0.52, 0.54, 0.55, 0.56, 0.55],
        },
    )
    resp = await collect_market_opinion(req)
    for s in resp.signals:
        print(f"{s.agent}: p={s.probability} conf={s.confidence:.2f} caveats={s.caveats[:3]}")
        if s.inputs_used:
            print("  inputs:", [i.source for i in s.inputs_used])
    print("elapsed_ms", resp.elapsed_ms)


if __name__ == "__main__":
    asyncio.run(main())
