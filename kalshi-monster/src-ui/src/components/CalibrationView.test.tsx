import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { CalibrationView } from './CalibrationView';
import { kalshiApi } from '../services/kalshi';
import { finceptApi } from '../services/tauri';

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    getForecastCalibrationReport: vi.fn(),
    resolvePendingForecasts: vi.fn(),
    analyzeTopMarketsEdge: vi.fn(),
    evaluateBreakers: vi.fn(),
    refitLambda: vi.fn(),
    getEdgeConfig: vi.fn(),
    manualReenableBreaker: vi.fn(),
  },
}));

vi.mock('../services/tauri', () => ({
  finceptApi: {
    getBridgeStatus: vi.fn(),
  },
}));

const defaultBreakers = {
  state: {
    stake_scaling_active: false,
    live_trading_disabled: false,
    paper_mode_forced: false,
  },
  live_orders_allowed: false,
  paper_only: false,
  stake_multiplier: 1,
  reasons: ['calibration gate locked'],
};

describe('CalibrationView', () => {
  beforeEach(() => {
    vi.mocked(kalshiApi.getForecastCalibrationReport).mockResolvedValue({
      resolved_count: 3,
      unresolved_count: 8,
      brier_market: 0.21,
      brier_final: 0.19,
      brier_model: 0.18,
      brier_market_on_model_rows: 0.22,
      n_model: 2,
      gate_passed: false,
      gate_reasons: [
        '3 resolved forecasts ≥ 200 required: NOT met',
        'Brier(p_final) 0.1900 ≤ Brier(p_market) 0.2100: met',
        'paper P&L after fees -1.00 > 0: NOT met',
      ],
      paper_pnl: -1,
      reliability_final: [
        { predicted_mean: 0.15, observed_freq: 0.2, count: 5 },
        { predicted_mean: 0.55, observed_freq: 0.5, count: 3 },
      ],
      reliability_market: [
        { predicted_mean: 0.15, observed_freq: 0.18, count: 5 },
        { predicted_mean: 0.55, observed_freq: 0.52, count: 3 },
      ],
    });
    vi.mocked(finceptApi.getBridgeStatus).mockResolvedValue({
      online: true,
      degraded: false,
      restarts_remaining: 3,
    });
    vi.mocked(kalshiApi.evaluateBreakers).mockResolvedValue(defaultBreakers);
    vi.mocked(kalshiApi.getEdgeConfig).mockResolvedValue({
      shrinkage_lambda: 0.25,
      min_edge: 0.05,
      fee_multiplier: 0.07,
      kelly_fraction: 0.25,
      min_confidence: 0.3,
    });
    vi.mocked(kalshiApi.resolvePendingForecasts).mockResolvedValue(0);
    vi.mocked(kalshiApi.refitLambda).mockResolvedValue(null);
    vi.mocked(kalshiApi.analyzeTopMarketsEdge).mockResolvedValue([
      {
        forecast_id: 1,
        market_ticker: 'KXTEST',
        p_market: 0.5,
        p_model: 0.55,
        p_final: 0.52,
        confidence: 0.4,
        verdict: 'pass',
        verdict_reasons: ['edge below threshold'],
        edge_net_yes: 0.01,
        edge_net_no: -0.02,
        signals_received: 5,
        signals_opining: 2,
        sidecar_elapsed_ms: 12,
      },
    ]);
  });

  test('loads gate report and shows locked gate with Brier numbers', async () => {
    render(<CalibrationView />);

    await waitFor(() => {
      expect(screen.getByText('LOCKED')).toBeInTheDocument();
    });
    expect(screen.getByText('3 / 200')).toBeInTheDocument();
    expect(screen.getByText('0.2100')).toBeInTheDocument();
    expect(screen.getByText('0.1900')).toBeInTheDocument();
    expect(screen.getByText(/Online/)).toBeInTheDocument();
    expect(screen.getByLabelText('p_final vs outcomes reliability chart')).toBeInTheDocument();
  });

  test('analyze top logs batch into table', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Analyze top 10/i }));

    await waitFor(() => {
      expect(screen.getByText('KXTEST')).toBeInTheDocument();
    });
    expect(kalshiApi.analyzeTopMarketsEdge).toHaveBeenCalledWith(10);
  });

  test('lambda re-fit surfaces insufficient-sample message', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Re-fit λ from ledger/i }));

    await waitFor(() => {
      expect(kalshiApi.refitLambda).toHaveBeenCalled();
    });
    expect(screen.getByText(/Not enough resolved forecasts/i)).toBeInTheDocument();
  });

  test('resolve settled forecasts refreshes report after IPC', async () => {
    vi.mocked(kalshiApi.resolvePendingForecasts).mockResolvedValue(2);
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());
    const callsBefore = vi.mocked(kalshiApi.getForecastCalibrationReport).mock.calls.length;

    fireEvent.click(screen.getByRole('button', { name: /Resolve settled forecasts/i }));

    await waitFor(() => {
      expect(kalshiApi.resolvePendingForecasts).toHaveBeenCalled();
    });
    expect(screen.getByText(/Resolved 2 forecast row/i)).toBeInTheDocument();
    await waitFor(() => {
      expect(vi.mocked(kalshiApi.getForecastCalibrationReport).mock.calls.length).toBeGreaterThan(
        callsBefore,
      );
    });
  });
});