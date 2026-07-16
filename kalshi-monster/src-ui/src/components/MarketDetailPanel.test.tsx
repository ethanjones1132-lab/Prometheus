import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { MarketDetailPanel } from './MarketDetailPanel';
import { kalshiApi } from '../services/kalshi';
import { bankrollApi, configApi } from '../services/tauri';
import type { KalshiMarketSummary } from '../types/kalshi';

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    computeStakeAdjustment: vi.fn(),
    getCalibrationStatus: vi.fn(),
    getPriceHistory: vi.fn(),
    recordPaperDecision: vi.fn(),
    analyzeMarketEdge: vi.fn(),
  },
}));

vi.mock('../services/tauri', () => ({
  configApi: {
    get: vi.fn(),
    save: vi.fn(),
  },
  bankrollApi: {
    getConfig: vi.fn(),
  },
}));

const market: KalshiMarketSummary = {
  ticker: 'KX-CPI-JUN',
  event_ticker: 'KX-CPI',
  title: 'Will CPI come in above expectations?',
  category: 'Economics',
  status: 'open',
  yes_prob_pct: 60,
  yes_ask: 0.62,
  yes_bid: 0.59,
  no_ask: 0.41,
  no_bid: 0.38,
  last_price: 0.6,
  volume_24h: 50000,
  total_volume: 250000,
  liquidity: 10000,
  spread: 0.03,
  close_time: '2026-06-30T14:00:00Z',
  expiration_time: '2026-07-01T14:00:00Z',
  result: '',
  can_close_early: true,
  is_provisional: true,
};

describe('MarketDetailPanel', () => {
  beforeEach(() => {
    vi.mocked(configApi.get).mockResolvedValue({
      openrouter_api_key: '',
      llm_provider: 'openrouter',
      opencode_api_key: '',
      openrouter_base_url: 'https://openrouter.ai/api/v1',
      selected_model: 'model',
      system_prompt: '',
      max_context_players: 50,
      openweathermap_api_key: '',
      api_sports_key: '',
      brave_api_key: '',
      fred_api_key: '',
      risk_tolerance: 'moderate',
      preferred_leagues: ['NFL'],
      stat_weighting: 'balanced',
      output_format: 'json_plus_text',
      theme: 'dark',
      kalshi_email: '',
      kalshi_password: '',
      kalshi_poll_interval_secs: 60,
      max_bet_pct: 0.05,
      discord_webhook_url: '',
      telegram_bot_token: '',
      telegram_chat_id: '',
      bot_daily_picks_enabled: true,
      bot_game_alerts_enabled: true,
      bot_grading_results_enabled: true,
      bot_daily_picks_time: '08:00',
    });
    vi.mocked(bankrollApi.getConfig).mockResolvedValue({
      total_bankroll: 1000,
      initial_bankroll: 1000,
      kelly_fraction: 0.25,
      max_bet_pct: 0.05,
      min_bet: 1,
      default_odds: -110,
      strategy: 'kelly',
      player_risk_multipliers: {},
      daily_bet_limit: 100,
      weekly_bet_limit: 400,
      historical_brier: 0.129,
    });
    vi.mocked(kalshiApi.computeStakeAdjustment).mockResolvedValue({
      kelly_scale: 1,
      raw_recommended_stake: 25,
      adjusted_recommended_stake: 25,
      conflicts: [],
      warnings: [],
    });
    vi.mocked(kalshiApi.getCalibrationStatus).mockResolvedValue({
      raw_pct: 60,
      calibrated_pct: 58.2,
      adjustment_pct: -1.8,
      applied: true,
      artifact_kind: 'isotonic',
      n_fit: 420,
      source: 'embedded',
      volatility_haircut_pct: 7.5,
      category_sample_status: 'shared calibrator',
    });
    vi.mocked(kalshiApi.getPriceHistory).mockResolvedValue({
      ticker: market.ticker,
      snapshots: [],
    });
    vi.mocked(kalshiApi.recordPaperDecision).mockResolvedValue({
      prediction_id: 'paper-123',
      lot_opened: true,
      lot_id: 'lot-abc',
      final_decision: 'TAKE',
      contract_side: 'YES',
      ticker: market.ticker,
      stake: 25,
      price_to_enter: 0.55,
      demotion_notes: [],
      paper_lots_blocked: false,
    });
    vi.mocked(kalshiApi.analyzeMarketEdge).mockResolvedValue({
      forecast_id: 42,
      market_ticker: market.ticker,
      p_market: 0.6,
      p_model: 0.65,
      p_final: 0.62,
      confidence: 0.4,
      verdict: 'pass',
      verdict_reasons: ['edge below threshold'],
      edge_net_yes: 0.01,
      edge_net_no: -0.02,
      signals_received: 5,
      signals_opining: 1,
      sidecar_elapsed_ms: 20,
    });
  });

  test('shows market mechanics, risk flags, and explicit decision actions', async () => {
    render(<MarketDetailPanel market={market} onClose={vi.fn()} />);

    expect(await screen.findByText('Market mechanics')).toBeInTheDocument();
    expect(screen.getByText('Early close')).toBeInTheDocument();
    expect(screen.getByText('Provisional')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Record YES' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Watch' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Pass' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Run edge engine' })).toBeInTheDocument();
  });

  test('runs edge engine and surfaces ledger summary', async () => {
    render(<MarketDetailPanel market={market} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Run edge engine' }));
    await waitFor(() => {
      expect(kalshiApi.analyzeMarketEdge).toHaveBeenCalledWith('KX-CPI-JUN');
    });
    expect(await screen.findByText(/Ledger #42/i)).toBeInTheDocument();
  });

  test('records a paper decision once fair value creates positive edge', async () => {
    render(<MarketDetailPanel market={market} onClose={vi.fn()} />);

    fireEvent.change(await screen.findByLabelText(/fair probability/i), {
      target: { value: '70' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Record YES' }));

    await waitFor(() => {
      expect(kalshiApi.recordPaperDecision).toHaveBeenCalledWith(
        'paper-sim',
        expect.objectContaining({
          ticker: 'KX-CPI-JUN',
          contract_side: 'YES',
          decision: 'TAKE',
        }),
      );
    });
  });

  test('shows Kalshi-native calibration and volatility-adjusted Kelly context', async () => {
    render(<MarketDetailPanel market={market} onClose={vi.fn()} />);

    expect(await screen.findByText('Calibration & ML')).toBeInTheDocument();
    expect(screen.getByText('Calibrated 58.2%')).toBeInTheDocument();
    expect(screen.getByText('Raw 60.0%')).toBeInTheDocument();
    expect(screen.getByText('Volatility haircut 7.5%')).toBeInTheDocument();
    expect(screen.getByText('isotonic / embedded')).toBeInTheDocument();
  });
});
