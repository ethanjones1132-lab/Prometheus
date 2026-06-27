export interface KalshiMarket {
  ticker: string;
  title: string;
  subtitle?: string;
  category?: string;
  status: string;
  volume: number;
  open_interest?: number;
  yes_bid?: number;
  yes_ask?: number;
  no_bid?: number;
  no_ask?: number;
  last_price?: number;
  close_date?: string;
  settlement_date?: string;
}

export type PropSport = 'nba' | 'nfl' | 'mlb' | 'nhl' | 'other';

export interface PropPick {
  id: string;
  sport: PropSport;
  league: string;
  game: string;
  game_time: string;
  player: string;
  team: string;
  prop_type: string;
  line: number;
  projection: number;
  implied_probability: number;
  model_probability: number;
  edge_pct: number;
  confidence: number;
  risk: 'low' | 'medium' | 'high' | string;
  recommendation: string;
  reasoning: string;
  source: string;
  updated_at: string;
}

export interface EdgeAnalysisInput {
  player_name: string;
  stat_category: string;
  line: number;
  pick_type: string;
  projection: number;
  season_avg: number;
  last3_avg: number;
  home_avg?: number;
  away_avg?: number;
  is_home: boolean;
  defense_rank?: number;
  pace_rank?: number;
  usage_rate?: number;
  opponent_pace_rank?: number;
  park_factor?: number;
  goalie_quality_rank?: number;
  consistency_score?: number;
}

export interface ScoredProp {
  player_name: string;
  stat_category: string;
  line: number;
  pick_type: string;
  composite_score: number;
  edge_score: number;
  matchup_score: number;
  consistency_score: number;
  value_score: number;
  tier: string;
  win_probability: number;
  expected_value: number;
  kelly_stake_pct: number;
  confidence: string;
  key_factors: string[];
  risks: string[];
  recommendation: string;
}

export interface PropAnalysisResult {
  edge: {
    edge_pct: number;
    win_probability: number;
    expected_value: number;
    quality_tier: string;
    confidence: string;
  };
  scored: ScoredProp;
}

export interface AnalysisContext {
  scored_props: ScoredProp[];
}

export interface PropScoreInput {
  line: number;
  projection: number;
  implied_probability: number;
  model_probability: number;
  confidence: number;
}

export interface PropScore {
  edge_pct: number;
  expected_value_pct: number;
  risk: string;
  recommendation: string;
  reasoning: string;
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  reasoning?: string;
  timestamp: string;
  tokens_used?: number;
}

export interface ChatSession {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  model: string;
  message_count: number;
  total_tokens: number;
}

export interface OpenRouterResponse {
  content: string;
  reasoning?: string;
  tokens_used?: number;
  model: string;
}

export interface PredictionRecord {
  id: string;
  ticker: string;
  market_title: string;
  category: string;
  pick: string;
  confidence: number;
  win_probability: number;
  edge_pct: number;
  stake: number;
  result: string;
  profit_loss: number;
  created_at: string;
  resolved_at?: string;
  notes?: string;
}

export interface AppConfig {
  openrouter_api_key: string;
  openrouter_base_url: string;
  selected_model: string;
  system_prompt: string;
  max_context_players: number;
  openweathermap_api_key: string;
  api_sports_key: string;
  risk_tolerance: string;
  preferred_leagues: string[];
  stat_weighting: string;
  output_format: string;
  theme: string;
  kalshi_email: string;
  kalshi_password: string;
  kalshi_poll_interval_secs: number;
  max_bet_pct: number;
  discord_webhook_url: string;
  telegram_bot_token: string;
  telegram_chat_id: string;
  bot_daily_picks_enabled: boolean;
  bot_game_alerts_enabled: boolean;
  bot_grading_results_enabled: boolean;
  bot_daily_picks_time: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  context_window: number;
  description: string;
  speed: string;
  cost: string;
}

export interface ApiStatus {
  connected: boolean;
  model_available: boolean;
  credits_remaining?: string;
  error?: string;
}

export interface SecurityPosture {
  csp_enforced: boolean;
  secrets_redacted: boolean;
  config_file_contains_secrets: boolean;
  secret_store: string;
  redacted_fields: string[];
  warnings: string[];
}

export interface BankrollConfig {
  total_bankroll: number;
  initial_bankroll: number;
  kelly_fraction: number;
  max_bet_pct: number;
  min_bet: number;
  default_odds: number;
  strategy: string;
  player_risk_multipliers: Record<string, number>;
  daily_bet_limit: number;
  weekly_bet_limit: number;
  historical_brier: number;
}

export interface BankrollSummary {
  config: BankrollConfig;
  roi_pct: number;
  total_wagered: number;
  total_won: number;
  total_lost: number;
  profit_loss: number;
  net_profit: number;
  current_bankroll: number;
  bets_placed: number;
  win_rate: number;
  bets_today: number;
  bets_this_week: number;
  remaining_daily: number;
  remaining_weekly: number;
  daily_limit_used: number;
  weekly_limit_used: number;
  prediction_open_exposure: number;
  paper_open_exposure: number;
  paper_cash_balance: number;
  paper_realized_pnl: number;
  synced_at: string;
}

export interface DataSourceStatus {
  prizepicks_props_available: boolean;
  prizepicks_seed_count: number;
  kalshi_connected: boolean;
  kalshi_index_size: number;
  openrouter_configured: boolean;
}

export interface MLCategoryStat {
  category: string;
  resolved_count: number;
  pending_count: number;
  trainable: boolean;
  samples_until_trainable: number;
  min_resolved_for_sidecar: number;
}

export interface MLPerCategoryModel {
  samples: number;
  cv_accuracy_mean?: number;
  model_exists: boolean;
}

export interface MLModelStatus {
  model_exists: boolean;
  model_path: string;
  trained_at?: string;
  samples?: number;
  cv_accuracy_mean?: number;
  pending_predictions: number;
  resolved_predictions: number;
  category_stats: MLCategoryStat[];
  per_category_models?: Record<string, MLPerCategoryModel>;
  message: string;
}

export interface NotificationSettings {
  enabled: boolean;
  game_starting_enabled: boolean;
  game_final_enabled: boolean;
  prediction_graded_enabled: boolean;
  grading_complete_enabled: boolean;
  kalshi_notifications_enabled: boolean;
  poll_interval_secs: number;
  game_starting_minutes_before: number;
  show_os_notifications: boolean;
}
