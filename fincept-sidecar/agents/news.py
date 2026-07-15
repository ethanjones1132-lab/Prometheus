# News agent — resolution-aware heuristic over structured web snippets (plan Phase B2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Honesty rules:
#   - probability=None unless context.web_snippets provides grounded text
#   - Never invent catalysts or polls from the title alone
#   - Shifts are small and confidence is low (snippets ≠ calibrated model)

from __future__ import annotations

import re
from datetime import datetime, timezone
from typing import Any

from fincept_sidecar.schemas import AgentSignal, CatalystEvent, DataRef, MarketOpinionRequest

# Directional lexicons — crude but testable; only applied when snippets exist.
_YES_LEAN = (
    "wins",
    "won",
    "victory",
    "leading",
    "leads",
    "ahead",
    "passes",
    "passed",
    "approved",
    "confirms",
    "confirmed",
    "surges",
    "beats",
    "beat",
    "higher than expected",
    "above expectations",
    "secures",
    "clinches",
)
_NO_LEAN = (
    "loses",
    "lost",
    "defeat",
    "trailing",
    "fails",
    "failed",
    "rejected",
    "denies",
    "denied",
    "drops",
    "misses",
    "missed",
    "below expectations",
    "withdraws",
    "suspended",
    "indicted",
)


def _snippet_blob(req: MarketOpinionRequest) -> list[dict[str, str]]:
    ctx = req.context or {}
    raw = ctx.get("web_snippets") or ctx.get("snippets") or []
    out: list[dict[str, str]] = []
    if not isinstance(raw, list):
        return out
    for item in raw:
        if not isinstance(item, dict):
            continue
        title = str(item.get("title") or "").strip()
        url = str(item.get("url") or "").strip()
        snippet = str(item.get("snippet") or item.get("description") or "").strip()
        if not title and not snippet:
            continue
        out.append({"title": title, "url": url, "snippet": snippet})
    return out


def _score_direction(text: str) -> float:
    """Return lean in [-1, 1] from keyword counts (YES positive)."""
    t = text.lower()
    yes = sum(1 for k in _YES_LEAN if k in t)
    no = sum(1 for k in _NO_LEAN if k in t)
    total = yes + no
    if total == 0:
        return 0.0
    return (yes - no) / total


def _extract_catalysts(text: str, close_time: datetime) -> list[CatalystEvent]:
    """Best-effort date mentions inside the contract window."""
    cats: list[CatalystEvent] = []
    # ISO-ish dates
    for m in re.finditer(r"\b(20\d{2})-(\d{2})-(\d{2})\b", text):
        try:
            dt = datetime(int(m.group(1)), int(m.group(2)), int(m.group(3)), tzinfo=timezone.utc)
            if dt <= close_time:
                cats.append(
                    CatalystEvent(
                        description=f"Date mentioned in evidence: {m.group(0)}",
                        occurs_at=dt,
                        source="news:snippet_date",
                    )
                )
        except ValueError:
            continue
    return cats[:3]


def estimate_sync(req: MarketOpinionRequest) -> AgentSignal:
    """Pure/sync path for tests — same logic as async estimate."""
    snippets = _snippet_blob(req)
    if not snippets:
        return AgentSignal(
            agent="news",
            probability=None,
            confidence=0.0,
            rationale=(
                "No web_snippets in context — refusing to invent a news-based probability. "
                "Rust should pass structured search hits when available."
            ),
            inputs_used=[],
            caveats=["missing:web_snippets"],
        )

    blob_parts = [f"{s['title']} {s['snippet']}" for s in snippets]
    blob = "\n".join(blob_parts)
    lean = _score_direction(blob)
    market_mid = max(0.01, min(0.99, (req.yes_bid + req.yes_ask) / 2.0))

    if abs(lean) < 0.15:
        return AgentSignal(
            agent="news",
            probability=None,
            confidence=0.0,
            rationale=(
                f"Received {len(snippets)} snippet(s) but directional language is weak "
                f"(lean={lean:.2f}); no opinion rather than a coin-flip shift."
            ),
            inputs_used=[
                DataRef(
                    source=f"web_snippets:n={len(snippets)}",
                    fetched_at=datetime.now(timezone.utc),
                )
            ],
            caveats=["evidence_inconclusive"],
        )

    # Small, capped shift from market mid — snippets are not a calibrated model.
    shift = 0.06 * lean  # max ±6 pts
    p = max(0.01, min(0.99, market_mid + shift))
    conf = min(0.35, 0.12 + 0.08 * abs(lean) + 0.03 * min(len(snippets), 5))

    return AgentSignal(
        agent="news",
        probability=p,
        confidence=conf,
        rationale=(
            f"Snippet heuristic lean={lean:.2f} over {len(snippets)} grounded hit(s); "
            f"shifted market mid {market_mid:.3f} by {shift:+.3f} → p={p:.3f}. "
            "Low confidence — treat as catalyst prior, not fair value."
        ),
        inputs_used=[
            DataRef(
                source=s.get("url") or f"web_snippet:{i}",
                fetched_at=datetime.now(timezone.utc),
            )
            for i, s in enumerate(snippets[:5])
        ],
        caveats=["heuristic_only", "not_calibrated"],
    )


async def estimate(req: MarketOpinionRequest) -> AgentSignal:
    return estimate_sync(req)


def extract_catalysts_from_request(req: MarketOpinionRequest) -> list[CatalystEvent]:
    snippets = _snippet_blob(req)
    if not snippets:
        return []
    blob = "\n".join(f"{s['title']} {s['snippet']}" for s in snippets)
    close = req.close_time
    if close.tzinfo is None:
        close = close.replace(tzinfo=timezone.utc)
    return _extract_catalysts(blob, close)


# Silence unused import if Any needed later
_ = Any
