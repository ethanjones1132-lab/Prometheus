export interface KalshiMarketSummary {
  ticker: string;
  event_ticker: string;
  title: string;
  category: string;
  status: string;
  yes_prob_pct: number;
  yes_ask: number;
  yes_bid: number;
  no_ask: number;
  no_bid: number;
  last_price: number;
  volume_24h: number;
  total_volume: number;
  liquidity: number;
  spread: number;
  close_time?: string | null;
  expiration_time?: string | null;
  result: string;
  can_close_early: boolean;
  is_provisional: boolean;
}

export interface KalshiCategoryStat {
  category: string;
  count: number;
  volume_24h: number;
}

export interface MLPhase3DashboardSummary {
  trainable_non_sports_categories: number;
  non_sports_sidecar_target: number;
  phase_3_data_metric_ready: boolean;
  kalshi_resolved_predictions: number;
  kalshi_pending_predictions: number;
  next_sidecar_category?: string | null;
  next_sidecar_samples_needed?: number | null;
  auto_retrain_eligible?: boolean;
  resolved_until_auto_retrain?: number;
  unified_model_on_disk?: boolean;
  active_sidecar_count?: number;
}

export interface KalshiDashboardBootstrap {
  markets: KalshiMarketSummary[];
  categories: KalshiCategoryStat[];
  cache_status: 'cold' | 'partial' | 'full' | string;
  cache_age_secs?: number | null;
  partial_catalog: boolean;
  last_refresh_at?: string | null;
  market_count: number;
  category_count: number;
  dashboard_generated_at: string;
  data_quality_notes: string[];
  ml_phase3?: MLPhase3DashboardSummary | null;
}

export interface KalshiPrediction {
  id: string;
  ticker: string;
  title: string;
  category: string;
  predicted_probability: number;
  actual_outcome?: string | null;
  confidence_score?: number | null;
  reasoning?: string | null;
  created_at: string;
  resolved_at?: string | null;
  stake_amount: number;
  pnl?: number | null;
  pick_type?: string | null;
  price_to_enter?: number | null;
  market_price_at_entry?: number | null;
  contract_side?: string | null;
  edge_points?: number | null;
  fractional_kelly_pct?: number | null;
  recommended_stake_dollars?: number | null;
  risk_flags?: string[] | null;
  thesis?: string | null;
  data_quality?: string | null;
  decision?: string | null;
}

export interface CorrelationConflict {
  exposure_ticker: string;
  exposure_title: string;
  strength: string;
  kelly_multiplier: number;
  explanation: string;
}

export interface StakeAdjustment {
  kelly_scale: number;
  raw_recommended_stake: number;
  adjusted_recommended_stake: number;
  conflicts: CorrelationConflict[];
  warnings: string[];
  remaining_daily?: number;
  remaining_weekly?: number;
  bankroll_cap?: number;
}

export interface CalibrationStatus {
  raw_pct: number;
  calibrated_pct: number;
  adjustment_pct: number;
  applied: boolean;
  artifact_kind: string;
  n_fit: number;
  source: string;
  volatility_haircut_pct: number;
  category_sample_status: string;
}

export interface KalshiPriceSnapshot {
  id: string;
  ticker: string;
  title: string;
  category: string;
  yes_prob_pct: number;
  yes_bid: number;
  yes_ask: number;
  spread: number;
  volume_24h: number;
  liquidity: number;
  snapshot_at: string;
}

export interface KalshiPriceHistory {
  ticker: string;
  snapshots: KalshiPriceSnapshot[];
  opening_yes_prob?: number | null;
  current_yes_prob?: number | null;
  prob_change?: number | null;
  spread_change?: number | null;
}

export interface KalshiTradeDecision {
  ticker: string;
  market_title: string;
  category: string;
  contract_side: 'YES' | 'NO' | 'PASS';
  market_price_pct: number;
  fair_probability_pct: number;
  edge_points: number;
  spread_cents: number;
  liquidity_score: number;
  ev_per_contract_cents: number;
  ev_roi_pct: number;
  raw_kelly_pct: number;
  fractional_kelly_pct: number;
  recommended_stake_dollars: number;
  max_position_dollars: number;
  decision: 'TAKE' | 'WATCH' | 'PASS';
  confidence_tier: 'High' | 'Medium' | 'Low' | 'None';
  thesis: string;
  evidence: string[];
  risk_flags: string[];
  data_quality: string;
  price_to_enter: number;
}

export type KalshiBetSide = 'YES' | 'NO' | 'PASS' | 'UNKNOWN';

export function parseKalshiBetSide(
  contractSide?: string | null,
  pickType?: string | null,
): KalshiBetSide {
  const side = (contractSide ?? '').trim().toUpperCase();
  if (side === 'YES') return 'YES';
  if (side === 'NO') return 'NO';
  if (side === 'PASS') return 'PASS';
  const pick = (pickType ?? '').trim().toLowerCase();
  if (pick === 'over') return 'YES';
  if (pick === 'under') return 'NO';
  return 'UNKNOWN';
}

export interface PaperAnalytics {
  starting_balance: number;
  cash_balance: number;
  open_market_value: number;
  equity: number;
  realized_pnl: number;
  unrealized_pnl: number;
  total_return_pct: number;
  total_trades: number;
  open_positions: number;
  win_rate: number;
  wins: number;
  losses: number;
  profit_factor: number;
  max_drawdown_pct: number;
  fetched_at: string;
}

export interface PaperPosition {
  ticker: string;
  title: string;
  category: string;
  side: string;
  total_qty: number;
  avg_entry_price_cents: number;
  cost_basis_dollars: number;
  mark_price_cents?: number | null;
  market_value_dollars?: number | null;
  unrealized_pnl_dollars?: number | null;
  lots_count: number;
}

export interface PaperSettlementSummary {
  settled: number;
  wins: number;
  losses: number;
  total_pnl: number;
  details?: Array<{
    lot_id: string;
    ticker: string;
    side: string;
    result: string;
    realized_pnl: number;
  }>;
  fetched_at?: string;
}

export interface PaperAccount {
  id: number;
  balance_dollars: number;
  total_deposits: number;
  total_withdrawals: number;
  created_at: string;
  updated_at: string;
}

export function kalshiBetWon(pred: KalshiPrediction): boolean | null {
  const actual = pred.actual_outcome;
  if (!actual) return null;
  const side = parseKalshiBetSide(pred.contract_side, pred.pick_type);
  if (side === 'YES') return actual === 'Yes';
  if (side === 'NO') return actual === 'No';
  return null;
}
