# agents/ — AGPL analysis agents for fincept-sidecar (plan §5)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Agents produce AgentSignal opinions only. They never hold Kalshi credentials,
# never size stakes, and never place orders. Money decisions stay in the Rust core.

from .orchestrator import collect_market_opinion

__all__ = ["collect_market_opinion"]
