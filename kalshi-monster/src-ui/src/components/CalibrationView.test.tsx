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
    paperFromEdge: vi.fn(),
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
      eligible_count: 1,
      unresolved_count: 8,
      // The raw ledger flatters the model — p_final "beats" p_market — because
      // market-only rows have p_final = p_market by construction. The eligible
      // sample says the opposite. The dashboard must follow `eligible`.
      raw: {
        n: 3,
        n_model: 2,
        brier_market: 0.21,
        brier_final: 0.19,
        brier_model: 0.18,
        brier_market_on_model_rows: 0.22,
      },
      eligible: {
        n: 1,
        n_model: 1,
        brier_market: 0.1,
        brier_final: 0.3,
        brier_model: 0.35,
        brier_market_on_model_rows: 0.1,
      },
      gate_passed: false,
      gate_reasons: [
        '1 eligible forecasts ≥ 200 required: NOT met (of 3 resolved; excludes rows with no p_model, rows created after the event started, and duplicate legs of one event)',
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
    // The gate tracks the eligible sample, not the raw resolved count.
    expect(screen.getByText('1 / 200')).toBeInTheDocument();
    expect(screen.getByText('Resolved (raw)')).toBeInTheDocument();
    // Headline Brier tiles are the ELIGIBLE numbers (0.1000 / 0.3000), not the
    // raw ones (0.2100 / 0.1900).
    expect(screen.getAllByText('0.1000').length).toBeGreaterThan(0);
    expect(
      screen.getAllByText((_, el) => el?.textContent?.includes('0.3000') === true).length,
    ).toBeGreaterThan(0);
    // The raw pair is still shown, but explicitly labelled as raw.
    expect(
      screen.getAllByText((_, el) => el?.textContent?.includes('Raw ledger') === true).length,
    ).toBeGreaterThan(0);
    expect(screen.getByLabelText('Flywheel status')).toBeInTheDocument();
    expect(screen.getByLabelText('Gate dashboard')).toBeInTheDocument();
    expect(screen.getByText(/Online/)).toBeInTheDocument();
    expect(screen.getByLabelText('p_final vs outcomes reliability chart')).toBeInTheDocument();
  });

  /// The footgun this guards: raw p_final (0.19) beats raw p_market (0.21)
  /// purely because market-only rows set them equal, while the eligible sample
  /// (0.30 vs 0.10) says the model is worse. A "≤ mkt" badge here would be a
  /// green light computed from an identity.
  test('does not show a beats-market badge when only the raw ledger flatters the model', async () => {
    render(<CalibrationView />);

    await waitFor(() => {
      expect(screen.getByText('LOCKED')).toBeInTheDocument();
    });
    expect(
      screen.queryAllByText((_, el) => el?.textContent?.includes('≤ mkt') === true),
    ).toHaveLength(0);
  });

  /// λ re-fit readiness must count eligible model rows (1), not every row that
  /// happens to carry a p_model (2).
  test('lambda re-fit progress counts the eligible sample', async () => {
    render(<CalibrationView />);

    await waitFor(() => {
      expect(screen.getByText('LOCKED')).toBeInTheDocument();
    });
    expect(
      screen.getAllByText((_, el) => el?.textContent?.includes('1 / 50') === true).length,
    ).toBeGreaterThan(0);
  });

  test('edge board scan logs batch into ranked table', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Scan top 10/i }));

    await waitFor(() => {
      expect(screen.getByText('KXTEST')).toBeInTheDocument();
    });
    expect(kalshiApi.analyzeTopMarketsEdge).toHaveBeenCalledWith(10, false);
  });

  test('deep analyze top 3 calls IPC with deep flag', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Deep analyze top 3/i }));

    await waitFor(() => {
      expect(kalshiApi.analyzeTopMarketsEdge).toHaveBeenCalledWith(3, true);
    });
  });

  test('clicking edge board row opens agent drawer', async () => {
    vi.mocked(kalshiApi.analyzeTopMarketsEdge).mockResolvedValue([
      {
        forecast_id: 1,
        market_ticker: 'KXTEST',
        p_market: 0.5,
        p_model: 0.55,
        p_final: 0.52,
        confidence: 0.4,
        verdict: 'pass',
        verdict_reasons: ['silent agent weight 50% of routing (macro)'],
        agent_breakdown: {
          signals: [
            { agent: 'technical', probability: 0.55, confidence: 0.4, rationale: 'ok' },
            { agent: 'macro', probability: null, confidence: 0, rationale: 'no data' },
          ],
        },
        edge_net_yes: 0.01,
        edge_net_no: -0.02,
        signals_received: 2,
        signals_opining: 1,
        sidecar_elapsed_ms: 12,
      },
    ]);
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /Scan top 10/i }));
    await waitFor(() => expect(screen.getByText('KXTEST')).toBeInTheDocument());
    fireEvent.click(screen.getByText('KXTEST'));
    expect(await screen.findByLabelText('Agent breakdown drawer')).toBeInTheDocument();
    expect(screen.getByText('technical')).toBeInTheDocument();
    expect(screen.getByText(/silent agent weight/i)).toBeInTheDocument();
  });

  test('shows empty edge board hint when no batch yet', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());
    expect(screen.getByLabelText('Edge Board empty')).toBeInTheDocument();
  });

  test('lambda re-fit surfaces insufficient-sample message', async () => {
    render(<CalibrationView />);
    await waitFor(() => expect(screen.getByText('LOCKED')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /Re-fit λ/i }));

    await waitFor(() => {
      expect(kalshiApi.refitLambda).toHaveBeenCalled();
    });
    expect(screen.getByText(/Not enough resolved forecasts/i)).toBeInTheDocument();
    expect(screen.getByLabelText('Lambda sample progress')).toBeInTheDocument();
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