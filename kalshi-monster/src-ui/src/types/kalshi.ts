import type { MLCategoryStat } from './index';
import type { MLPerCategoryModel } from './index';

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

/** Output of edge_engine::pipeline::analyze_and_log_forecast (IPC). */
export interface EdgeAnalysisResult {
  forecast_id: number;
  market_ticker: string;
  p_market: number;
  p_model: number | null;
  p_final: number;
  confidence: number;
  verdict: string;
  verdict_reasons: string[];
  agent_breakdown?: unknown;
  edge_net_yes: number;
  edge_net_no: number;
  signals_received: number;
  signals_opining: number;
  sidecar_elapsed_ms?: number | null;
}

/** Forecast-ledger calibration gate report (Phase 3). */
export interface ForecastCalibrationReport {
  /** Every resolved row, including market-only and in-play ones. */
  resolved_count: number;
  /**
   * Rows that can actually testify to model skill: p_model present, created
   * before the event started, deduplicated to one per underlying event.
   * This is the number the gate tests against 200.
   */
  eligible_count: number;
  unresolved_count: number;
  brier_market: number | null;
  brier_final: number | null;
  brier_model: number | null;
  brier_market_on_model_rows: number | null;
  n_model: number;
  gate_passed: boolean;
  gate_reasons: string[];
  paper_pnl: number | null;
  reliability_final: ReliabilityBucket[];
  reliability_market: ReliabilityBucket[];
}

export interface ReliabilityBucket {
  predicted_mean: number;
  observed_freq: number;
  count: number;
}

export interface LiveOrderEligibility {
  allowed: boolean;
  calibration_gate_passed: boolean;
  breakers_live_orders_allowed: boolean;
  reasons: string[];
}

/** §6.4 circuit breaker latch state (persisted). */
export interface BreakerState {
  stake_scaling_active: boolean;
  live_trading_disabled: boolean;
  paper_mode_forced: boolean;
}

/** Outcome of one breaker evaluation tick. */
export interface BreakerDecision {
  state: BreakerState;
  live_orders_allowed: boolean;
  paper_only: boolean;
  stake_multiplier: number;
  reasons: string[];
}

/** Persisted edge-engine tunables (IPC). */
export interface EdgeConfig {
  shrinkage_lambda: number;
  min_edge: number;
  fee_multiplier: number;
  kelly_fraction: number;
  min_confidence: number;
}

/** Result of plan §4.1 λ grid re-fit from resolved forecast ledger. */
export interface LambdaFit {
  lambda: number;
  brier_at_fit: number;
  brier_at_market: number;
  brier_at_model: number;
  n: number;
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
  non_sports_category_stats?: MLCategoryStat[];
  unified_cv_accuracy_mean?: number | null;
  unified_cv_accuracy_std?: number | null;
  unified_trained_at?: string | null;
  active_sidecar_models?: Record<string, MLPerCategoryModel> | null;
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
  /** Closing mid (0–1) when graded */
  close_price?: number | null;
  /** CLV = close − entry */
  clv?: number | null;
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
  /** Optional; inferred client-side when omitted (|fair − market| ≥ 15). */
  model_disagreement?: boolean;
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

/** Structured result from kalshi_record_paper_decision (Sprint 0.1). */
export interface PaperRecordResult {
  prediction_id: string;
  lot_opened: boolean;
  lot_id?: string | null;
  final_decision: string;
  contract_side: string;
  ticker: string;
  stake: number;
  price_to_enter: number;
  demotion_notes: string[];
  paper_lots_blocked: boolean;
}

export function kalshiBetWon(pred: KalshiPrediction): boolean | null {
  const actual = pred.actual_outcome;
  if (!actual) return null;
  const side = parseKalshiBetSide(pred.contract_side, pred.pick_type);
  if (side === 'YES') return actual === 'Yes';
  if (side === 'NO') return actual === 'No';
  return null;
}
