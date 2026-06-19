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

export interface PropFilterRequest {
  filter?: string;
  limit?: number;
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  reasoning?: string;
  timestamp: string;
  tokens_used?: number;
}

export interface ChatRequest {
  session_id: string;
  message: string;
  stream: boolean;
}

export interface ChatResponse {
  message: ChatMessage;
  session_id: string;
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

export interface RedactedConfig {
  openrouter_api_key_set: boolean;
  openrouter_base_url: string;
  selected_model: string;
  system_prompt: string;
  max_context_markets: number;
  risk_tolerance: string;
  preferred_categories: string[];
  market_weighting: string;
  output_format: string;
  theme: string;
  kalshi_email: string;
  kalshi_password_set: boolean;
  kalshi_poll_interval_secs: number;
  discord_webhook_url_set: boolean;
  telegram_chat_id: string;
  telegram_bot_token_set: boolean;
  bot_daily_picks_enabled: boolean;
  bot_grading_results_enabled: boolean;
  bot_daily_picks_time: string;
}

export interface DataSourceStatus {
  prizepicks_props_available: boolean;
  prizepicks_seed_count: number;
  kalshi_connected: boolean;
  kalshi_index_size: number;
  openrouter_configured: boolean;
}
