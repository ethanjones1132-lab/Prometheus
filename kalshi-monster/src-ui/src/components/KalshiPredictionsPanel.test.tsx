import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { KalshiPredictionsPanel } from './KalshiPredictionsPanel';
import { kalshiApi } from '../services/kalshi';

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    getPredictions: vi.fn(),
    getPaperAnalytics: vi.fn(),
    getPaperPositions: vi.fn(),
    settlePaperPositions: vi.fn(),
    resetPaperAccount: vi.fn(),
    gradePending: vi.fn(),
  },
}));

describe('KalshiPredictionsPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.mocked(kalshiApi.getPredictions).mockResolvedValue([]);
    vi.mocked(kalshiApi.getPaperAnalytics).mockResolvedValue({
      starting_balance: 10000,
      cash_balance: 9700,
      open_market_value: 340,
      equity: 10040,
      realized_pnl: 10,
      unrealized_pnl: 30,
      total_return_pct: 0.4,
      total_trades: 2,
      open_positions: 1,
      win_rate: 50,
      wins: 1,
      losses: 1,
      profit_factor: 1.4,
      max_drawdown_pct: 1.2,
      fetched_at: '2026-06-22T17:00:00Z',
    });
    vi.mocked(kalshiApi.getPaperPositions).mockResolvedValue([
      {
        ticker: 'KX-FED-DEC',
        title: 'Will the Fed cut rates in December?',
        category: 'Economics',
        side: 'YES',
        total_qty: 10,
        avg_entry_price_cents: 56,
        cost_basis_dollars: 5.6,
        mark_price_cents: 61,
        market_value_dollars: 6.1,
        unrealized_pnl_dollars: 0.5,
        lots_count: 1,
      },
    ]);
    vi.mocked(kalshiApi.settlePaperPositions).mockResolvedValue({
      settled: 1,
      wins: 1,
      losses: 0,
      total_pnl: 4.4,
    });
    vi.mocked(kalshiApi.resetPaperAccount).mockResolvedValue({
      id: 1,
      balance_dollars: 10000,
      total_deposits: 10000,
      total_withdrawals: 0,
      created_at: '2026-06-22T17:00:00Z',
      updated_at: '2026-06-22T17:00:00Z',
    });
    vi.mocked(kalshiApi.gradePending).mockResolvedValue({
      total_predictions: 0,
      pending_gradable: 0,
      graded: 0,
      wins: 0,
      losses: 0,
      total_pnl: 0,
      fetched_at: '2026-06-22T17:00:00Z',
    });
  });

  test('shows open paper positions with mark, value, and unrealized PnL', async () => {
    render(<KalshiPredictionsPanel />);

    expect(await screen.findByText('Paper portfolio')).toBeInTheDocument();
    expect(screen.getByText('KX-FED-DEC')).toBeInTheDocument();
    expect(screen.getByText('YES x10')).toBeInTheDocument();
    expect(screen.getByText('Entry 56.0c')).toBeInTheDocument();
    expect(screen.getByText('Mark 61.0c')).toBeInTheDocument();
    expect(screen.getByText('Value $6.10')).toBeInTheDocument();
    expect(screen.getByText('PnL $0.50')).toBeInTheDocument();
  });

  test('settles and resets the paper account through explicit controls', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(<KalshiPredictionsPanel />);

    fireEvent.click(await screen.findByRole('button', { name: 'Settle paper' }));

    await waitFor(() => {
      expect(kalshiApi.settlePaperPositions).toHaveBeenCalledTimes(1);
      expect(screen.getByText('Settled 1 (1W/0L, $4.40)')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Reset paper' }));

    await waitFor(() => {
      expect(kalshiApi.resetPaperAccount).toHaveBeenCalledWith(10000);
      expect(screen.getByText('Paper account reset to $10,000.00')).toBeInTheDocument();
    });
  });
});
