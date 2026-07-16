# fincept-sidecar — wire contracts (plan §5, §7 Phase 2, §14.4)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# These Pydantic models are the OpenAPI source of truth for the Rust bridge.
# The Rust `edge_engine::AgentSignal` mirrors AgentSignal field-for-field;
# change them in lock-step.

from __future__ import annotations

import enum
from datetime import datetime

from pydantic import BaseModel, Field


class MarketCategory(str, enum.Enum):
    """Routing categories (plan §5.2). Classified by the Rust side."""

    ECONOMIC = "economic"
    INDEX_PRICE_LEVEL = "index_price_level"
    COMPANY_EVENT = "company_event"
    POLITICAL = "political"
    OTHER = "other"


class DataRef(BaseModel):
    """A data input an agent used, with fetch timestamp — what makes the
    staleness check (plan §4.5) enforceable rather than decorative."""

    source: str
    fetched_at: datetime


class AgentSignal(BaseModel):
    """Universal agent output contract (plan §5).

    Agents MUST return probability=None rather than a hallucinated number
    when they lack relevant data — a sentiment agent has no business opining
    on initial jobless claims.
    """

    agent: str
    probability: float | None = Field(
        default=None,
        ge=0.01,
        le=0.99,
        description="P(YES) in [0.01, 0.99]; None = no opinion",
    )
    confidence: float = Field(ge=0.0, le=1.0, description="Self-assessed reliability")
    rationale: str = Field(description="2-5 sentences, shown to the user")
    inputs_used: list[DataRef] = Field(default_factory=list)
    caveats: list[str] = Field(default_factory=list)


class CatalystEvent(BaseModel):
    """A dated event inside the contract window (plan §5.1, News agent).

    A contract that looks mispriced with a catalyst tomorrow is a different
    bet from the same price with no catalyst.
    """

    description: str
    occurs_at: datetime
    source: str


class MarketOpinionRequest(BaseModel):
    """POST /api/v1/agents/market-opinion — the one endpoint that matters
    most (plan §7 Phase 2)."""

    market_ticker: str
    title: str
    resolution_rules: str = Field(
        description="Full resolution text; agents receive the exact criterion"
    )
    close_time: datetime
    category: MarketCategory
    yes_bid: float = Field(ge=0.0, le=1.0)
    yes_ask: float = Field(ge=0.0, le=1.0)
    context: dict = Field(
        default_factory=dict,
        description="Related tickers, open interest, recent contract candles",
    )


class MarketOpinionResponse(BaseModel):
    signals: list[AgentSignal]
    catalysts: list[CatalystEvent] = Field(default_factory=list)
    elapsed_ms: int


class AssetSignal(BaseModel):
    """Continuous-payoff contract for the stocks/crypto expansion
    (plan §14.4). Served by POST /api/v1/agents/asset-signal (Sprint 7.3);
    gated until binary calibration matures."""

    agent: str
    ticker: str
    horizon_days: int = Field(gt=0, description="Forecast horizon; 21 ≈ 1 month")
    expected_excess_return: float | None = Field(
        default=None, description="Annualized; None = no opinion"
    )
    return_vol: float = Field(gt=0.0, description="Annualized σ of the forecast distribution")
    confidence: float = Field(ge=0.0, le=1.0)
    rationale: str
    inputs_used: list[DataRef] = Field(default_factory=list)


class AssetSignalRequest(BaseModel):
    """POST /api/v1/agents/asset-signal — continuous book (gated)."""

    ticker: str = Field(description="Equity/crypto symbol, e.g. SPY or BTC-USD")
    horizon_days: int = Field(default=21, gt=0, le=365)
    # Caller (Rust) passes whether the binary calibration gate is open.
    calibration_gate_open: bool = Field(
        default=False,
        description="Must be true only when Kalshi forecast gate criteria pass",
    )
    # Optional injected closes for tests / offline; production may leave empty.
    closes: list[float] = Field(default_factory=list)


class HealthResponse(BaseModel):
    status: str
    uptime_seconds: float


class VersionResponse(BaseModel):
    version: str
    git_sha: str = Field(description="Embedded at build time for AGPL source traceability (plan §3 Rule 3)")
