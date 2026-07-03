import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { KalshiView } from './KalshiView';
import { kalshiApi } from '../services/kalshi';
import { mlApi } from '../services/tauri';
import type { KalshiMarketSummary } from '../types/kalshi';

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    getDashboardBootstrap: vi.fn(),
    searchMarkets: vi.fn(),
    getMarkets: vi.fn(),
    refresh: vi.fn(),
    getPredictions: vi.fn(),
    getPaperAnalytics: vi.fn(),
    getPaperPositions: vi.fn(),
    gradePending: vi.fn(),
    computeStakeAdjustment: vi.fn(),
    getCalibrationStatus: vi.fn(),
    getPriceHistory: vi.fn(),
    recordPaperDecision: vi.fn(),
    settlePaperPositions: vi.fn(),
    resetPaperAccount: vi.fn(),
  },
}));

vi.mock('../services/tauri', () => ({
  configApi: {
    get: vi.fn().mockResolvedValue({
      openrouter_api_key: '',
      openrouter_base_url: 'https://openrouter.ai/api/v1',
      selected_model: 'model',
      system_prompt: '',
      max_context_players: 50,
      openweathermap_api_key: '',
      api_sports_key: '',
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
    }),
    save: vi.fn(),
  },
  bankrollApi: {
    getConfig: vi.fn().mockResolvedValue({
      total_bankroll: 1000,
      current_bankroll: 1000,
      unit_size: 10,
      max_bet_pct: 0.05,
      daily_bet_limit: 100,
      weekly_bet_limit: 400,
      stop_loss_pct: 0.2,
      staking_strategy: 'kelly',
      kelly_fraction: 0.25,
      min_edge_pct: 2,
    }),
  },
  mlApi: {
    trainModel: vi.fn(),
  },
}));

const market: KalshiMarketSummary = {
  ticker: 'KX-FED-DEC',
  event_ticker: 'KX-FED',
  title: 'Will the Fed cut rates in December?',
  category: 'Economics',
  status: 'open',
  yes_prob_pct: 57.5,
  yes_ask: 0.59,
  yes_bid: 0.56,
  no_ask: 0.44,
  no_bid: 0.41,
  last_price: 0.57,
  volume_24h: 123456,
  total_volume: 500000,
  liquidity: 25000,
  spread: 0.03,
  close_time: '2026-12-12T20:00:00Z',
  expiration_time: '2026-12-13T20:00:00Z',
  result: '',
  can_close_early: false,
  is_provisional: false,
};

describe('KalshiView', () => {
  beforeEach(() => {
    vi.mocked(kalshiApi.getDashboardBootstrap).mockResolvedValue({
      markets: [market],
      categories: [{ category: 'Economics', count: 12, volume_24h: 123456 }],
      cache_status: 'partial',
      cache_age_secs: 17,
      partial_catalog: true,
      last_refresh_at: '2026-06-22T17:00:00Z',
      market_count: 1,
      category_count: 1,
      dashboard_generated_at: '2026-06-22T17:00:01Z',
      data_quality_notes: ['Partial catalog loaded for fast first paint'],
      ml_phase3: {
        trainable_non_sports_categories: 1,
        non_sports_sidecar_target: 3,
        phase_3_data_metric_ready: false,
        kalshi_resolved_predictions: 4,
        kalshi_pending_predictions: 2,
        next_sidecar_category: 'Politics',
        next_sidecar_samples_needed: 6,
        auto_retrain_eligible: true,
        resolved_until_auto_retrain: 0,
        unified_model_on_disk: true,
        active_sidecar_count: 1,
        unified_cv_accuracy_mean: 0.612,
        unified_cv_accuracy_std: 0.04,
        unified_trained_at: '2026-07-01T12:00:00Z',
        active_sidecar_models: {
          Economics: { samples: 14, cv_accuracy_mean: 0.58, model_exists: true },
        },
        non_sports_category_stats: [
          {
            category: 'Politics',
            resolved_count: 4,
            pending_count: 1,
            trainable: false,
            samples_until_trainable: 6,
            min_resolved_for_sidecar: 10,
          },
          {
            category: 'Economics',
            resolved_count: 10,
            pending_count: 0,
            trainable: true,
            samples_until_trainable: 0,
            min_resolved_for_sidecar: 10,
          },
          {
            category: 'Weather',
            resolved_count: 0,
            pending_count: 0,
            trainable: false,
            samples_until_trainable: 10,
            min_resolved_for_sidecar: 10,
          },
        ],
      },
    });
    vi.mocked(kalshiApi.getPredictions).mockResolvedValue([]);
    vi.mocked(kalshiApi.getPaperPositions).mockResolvedValue([]);
    vi.mocked(kalshiApi.getPaperAnalytics).mockResolvedValue({
      starting_balance: 10000,
      cash_balance: 10000,
      open_market_value: 0,
      equity: 10000,
      realized_pnl: 0,
      unrealized_pnl: 0,
      total_return_pct: 0,
      total_trades: 0,
      open_positions: 0,
      win_rate: 0,
      wins: 0,
      losses: 0,
      profit_factor: 0,
      max_drawdown_pct: 0,
      fetched_at: '2026-06-22T17:00:00Z',
    });
    vi.mocked(kalshiApi.computeStakeAdjustment).mockResolvedValue({
      kelly_scale: 1,
      raw_recommended_stake: 25,
      adjusted_recommended_stake: 25,
      conflicts: [],
      warnings: [],
    });
    vi.mocked(kalshiApi.getCalibrationStatus).mockResolvedValue({
      raw_pct: 57.5,
      calibrated_pct: 57.5,
      adjustment_pct: 0,
      applied: false,
      artifact_kind: 'none',
      n_fit: 0,
      source: 'none',
      volatility_haircut_pct: 0,
      category_sample_status: 'raw model probability',
    });
    vi.mocked(kalshiApi.getPriceHistory).mockResolvedValue({
      ticker: market.ticker,
      snapshots: [],
    });
  });

  test('shows market freshness and enterprise-grade card context', async () => {
    render(<KalshiView />);

    expect(await screen.findByText('Will the Fed cut rates in December?')).toBeInTheDocument();
    expect(screen.getByText('Partial catalog')).toBeInTheDocument();
    expect(screen.getByText('Cache age 17s')).toBeInTheDocument();
    expect(screen.getByText('Markets 1')).toBeInTheDocument();
    expect(screen.getByText('Categories 1')).toBeInTheDocument();
    expect(screen.getByText('Partial catalog loaded for fast first paint')).toBeInTheDocument();
    expect(
      screen.getByText(
        /ML Phase 3: 1\/3 sidecar categories · 4 resolved Kalshi paper rows · 2 pending grades · auto-retrain on grade active · next: Politics/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/ML artifacts: unified on disk, 1 sidecar · unified CV 61.2% ±4.0%/)).toBeInTheDocument();
    expect(screen.getByText('Sidecar data (Kalshi paper)')).toBeInTheDocument();
    expect(screen.getByText(/Unified model trained Jul 1, 2026/)).toBeInTheDocument();
    expect(screen.getByText(/Active sidecars: Economics \(14 samples, CV 58.0%\)/)).toBeInTheDocument();
    expect(screen.getByText(/10\/10 graded · sidecar ready/)).toBeInTheDocument();
    expect(screen.getByText('Status open')).toBeInTheDocument();
    expect(screen.getByText('Close Dec 12, 2026')).toBeInTheDocument();
    expect(screen.getByText('Liq $25,000')).toBeInTheDocument();
  });

  test('opens market detail from a dashboard card', async () => {
    render(<KalshiView />);

    fireEvent.click(await screen.findByRole('button', { name: /KX-FED-DEC/i }));

    await waitFor(() => {
      expect(screen.getByText('Market mechanics')).toBeInTheDocument();
    });
  });

  test('grades pending Kalshi rows from dashboard status strip', async () => {
    vi.mocked(kalshiApi.gradePending).mockResolvedValue({
      total_predictions: 10,
      pending_gradable: 2,
      graded: 2,
      wins: 1,
      losses: 1,
      total_pnl: 4.5,
      fetched_at: '2026-07-01T12:00:00Z',
    });

    render(<KalshiView />);

    const gradeBtn = await screen.findByRole('button', { name: /Grade 2 pending/i });
    fireEvent.click(gradeBtn);

    await waitFor(() => {
      expect(kalshiApi.gradePending).toHaveBeenCalledTimes(1);
      expect(screen.getByText(/Graded 2 \(1W\/1L, \$4\.50\)/)).toBeInTheDocument();
    });
  });

  test('surfaces snapshot and stale tape hints in decision tips', async () => {
    vi.mocked(kalshiApi.getDashboardBootstrap).mockResolvedValue({
      markets: [market],
      categories: [{ category: 'Economics', count: 12, volume_24h: 123456 }],
      cache_status: 'full',
      cache_age_secs: 120,
      partial_catalog: false,
      last_refresh_at: '2026-06-22T17:00:00Z',
      market_count: 1,
      category_count: 1,
      dashboard_generated_at: '2026-06-22T17:00:01Z',
      data_quality_notes: [
        'Full catalog cache ready',
        'Instant paint from saved market snapshot; live refresh runs in background',
        'Market tape is older than 60s — use Refresh and snapshot for live prices',
        'Live catalog refresh in progress — tape may update shortly',
      ],
      ml_phase3: null,
    });

    render(<KalshiView />);

    expect(
      await screen.findByText(/Tape is from a saved snapshot — refresh once live data lands/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Prices may be stale — hit Refresh and snapshot before recording paper trades/),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Instant paint from saved market snapshot; live refresh runs in background'),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Live catalog refresh is running — wait for the tape to settle/),
    ).toBeInTheDocument();
    expect(screen.getByText('Catalog updating')).toBeInTheDocument();
    expect(
      screen.getByText(/Live refresh is running — wait for the tape to settle before sizing/),
    ).toBeInTheDocument();
  });

  test('trains ML from dashboard when auto-retrain gate is satisfied', async () => {
    vi.mocked(mlApi.trainModel).mockResolvedValue({
      status: 'trained',
      samples: 42,
      cv_accuracy_mean: 0.61,
      message: 'ok',
    });

    render(<KalshiView />);

    const trainBtn = await screen.findByRole('button', { name: /Train ML models/i });
    fireEvent.click(trainBtn);

    await waitFor(() => {
      expect(mlApi.trainModel).toHaveBeenCalledTimes(1);
      expect(screen.getByText(/ML trained \(42 samples — CV 61\.0%\)/)).toBeInTheDocument();
    });
  });
});
