import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { SettingsView } from './SettingsView';
import { kalshiApi } from '../services/kalshi';
import { bankrollApi, configApi, mlApi, notificationApi } from '../services/tauri';

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    getEdgeConfig: vi.fn(),
    setEdgeConfig: vi.fn(),
  },
}));

vi.mock('../services/tauri', () => ({
  configApi: {
    get: vi.fn(),
    getAvailableModels: vi.fn(),
    getSecurityPosture: vi.fn(),
  },
  bankrollApi: {
    refreshHistoricalBrier: vi.fn(),
    getConfig: vi.fn(),
    getSummary: vi.fn(),
  },
  mlApi: {
    getModelStatus: vi.fn(),
  },
  notificationApi: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
  },
}));

const config = {
  openrouter_api_key: 'sk-or-v1-secret-value',
  llm_provider: 'openrouter',
  opencode_api_key: '',
  openrouter_base_url: 'https://openrouter.ai/api/v1',
  selected_model: 'model',
  system_prompt: '',
  max_context_players: 50,
  openweathermap_api_key: 'weather-secret',
  api_sports_key: 'sports-secret',
  brave_api_key: '',
  risk_tolerance: 'moderate',
  preferred_leagues: ['NFL'],
  stat_weighting: 'balanced',
  output_format: 'json_plus_text',
  theme: 'dark',
  kalshi_email: 'trader@example.com',
  kalshi_password: 'kalshi-secret',
  kalshi_poll_interval_secs: 60,
  max_bet_pct: 0.05,
  discord_webhook_url: 'https://discord.com/api/webhooks/secret',
  telegram_bot_token: 'telegram-secret',
  telegram_chat_id: '123',
  bot_daily_picks_enabled: true,
  bot_game_alerts_enabled: true,
  bot_grading_results_enabled: true,
  bot_daily_picks_time: '08:00',
};

describe('SettingsView', () => {
  beforeEach(() => {
    vi.mocked(configApi.get).mockResolvedValue(config);
    vi.mocked(configApi.getAvailableModels).mockResolvedValue([
      {
        id: 'model',
        name: 'Model',
        provider: 'Provider',
        context_window: 1000,
        description: 'Test model',
        speed: 'fast',
        cost: 'low',
      },
    ]);
    vi.mocked(configApi.getSecurityPosture).mockResolvedValue({
      csp_enforced: true,
      secrets_redacted: true,
      config_file_contains_secrets: true,
      secret_store: 'Local encrypted vault pending',
      redacted_fields: [
        'openrouter_api_key',
        'kalshi_password',
        'discord_webhook_url',
        'telegram_bot_token',
      ],
      warnings: ['Credential vault migration pending'],
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
    vi.mocked(bankrollApi.getSummary).mockResolvedValue({
      config: {
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
      },
      roi_pct: 0,
      total_wagered: 0,
      total_won: 0,
      total_lost: 0,
      profit_loss: 0,
      net_profit: 0,
      current_bankroll: 1000,
      bets_placed: 0,
      win_rate: 0,
      bets_today: 0,
      bets_this_week: 0,
      remaining_daily: 100,
      remaining_weekly: 400,
      daily_limit_used: 0,
      weekly_limit_used: 0,
      prediction_open_exposure: 0,
      paper_open_exposure: 0,
      paper_cash_balance: 10000,
      paper_realized_pnl: 0,
      synced_at: '2026-06-22T17:00:00Z',
    });
    vi.mocked(bankrollApi.refreshHistoricalBrier).mockResolvedValue(0.129);
    vi.mocked(mlApi.getModelStatus).mockResolvedValue({
      model_exists: false,
      model_path: '/tmp/ml_model.joblib',
      pending_predictions: 2,
      resolved_predictions: 5,
      category_stats: [
        {
          category: 'Sports',
          resolved_count: 5,
          pending_count: 2,
          trainable: false,
          samples_until_trainable: 5,
          min_resolved_for_sidecar: 10,
        },
      ],
      message: 'No model trained yet.',
    });
    vi.mocked(kalshiApi.getEdgeConfig).mockResolvedValue({
      shrinkage_lambda: 0.25,
      min_edge: 0.02,
      fee_multiplier: 1.0,
      kelly_fraction: 0.25,
      min_confidence: 0.5,
    });
    vi.mocked(kalshiApi.setEdgeConfig).mockResolvedValue({
      shrinkage_lambda: 0.3,
      min_edge: 0.02,
      fee_multiplier: 1.0,
      kelly_fraction: 0.25,
      min_confidence: 0.5,
    });
    vi.mocked(notificationApi.getSettings).mockResolvedValue({
      enabled: true,
      game_starting_enabled: true,
      game_final_enabled: true,
      prediction_graded_enabled: true,
      grading_complete_enabled: true,
      kalshi_notifications_enabled: true,
      poll_interval_secs: 60,
      game_starting_minutes_before: 30,
      show_os_notifications: true,
    });
  });

  test('shows redacted security posture without exposing secret values', async () => {
    render(<SettingsView />);

    expect(await screen.findByText('Security posture')).toBeInTheDocument();
    expect(screen.getByText('CSP enforced')).toBeInTheDocument();
    expect(screen.getByText('Secrets redacted from diagnostics')).toBeInTheDocument();
    expect(screen.getByText('Credential vault migration pending')).toBeInTheDocument();
    expect(screen.queryByText('sk-or-v1-secret-value')).not.toBeInTheDocument();
    expect(screen.queryByText('kalshi-secret')).not.toBeInTheDocument();
  });

  test('shows edge shrinkage lambda settings card', async () => {
    render(<SettingsView />);
    expect(await screen.findByText('Edge engine config')).toBeInTheDocument();
    expect(screen.getByLabelText(/Shrinkage λ/i)).toBeInTheDocument();
  });
});
