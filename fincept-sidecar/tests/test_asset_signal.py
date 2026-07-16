# SPDX-License-Identifier: AGPL-3.0-or-later
"""Sprint 7.3 — AssetSignal gated continuous path."""

from agents.asset_continuous import estimate_asset_signal


def test_gated_returns_null_excess():
    sig = estimate_asset_signal("SPY", calibration_gate_open=False, closes=[100.0] * 40)
    assert sig.expected_excess_return is None
    assert "gated" in sig.rationale.lower() or "calibration" in sig.rationale.lower()


def test_ungated_without_history_null():
    sig = estimate_asset_signal("SPY", calibration_gate_open=True, closes=None)
    assert sig.expected_excess_return is None


def test_ungated_with_history_may_opine():
    # Uptrend closes
    closes = [100.0 + i * 0.5 for i in range(40)]
    sig = estimate_asset_signal("SPY", calibration_gate_open=True, closes=closes)
    assert sig.return_vol > 0
    # Momentum should be non-null on a clear uptrend
    assert sig.expected_excess_return is not None
