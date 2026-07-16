import { invoke } from '@tauri-apps/api/core';
import type {
  KalshiCategoryStat,
  KalshiDashboardBootstrap,
  KalshiMarketSummary,
  KalshiPrediction,
  KalshiPriceHistory,
  KalshiTradeDecision,
  CalibrationStatus,
  EdgeAnalysisResult,
  ForecastCalibrationReport,
  BreakerDecision,
  LambdaFit,
  EdgeConfig,
  LiveOrderEligibility,
  PaperAccount,
  PaperAnalytics,
  PaperPosition,
  PaperRecordResult,
  PaperSettlementSummary,
  StakeAdjustment,
} from '../types/kalshi';

export interface KalshiGradingSummary {
  total_predictions: number;
  pending_gradable: number;
  graded: number;
  wins: number;
  losses: number;
  total_pnl: number;
  fetched_at: string;
}

export interface KalshiChatContextStatus {
  degraded: boolean;
  tape_market_count: number;
  reasons: string[];
}

export const kalshiApi = {
  getMarkets: (category: string) =>
    invoke<KalshiMarketSummary[]>('kalshi_get_markets', { category }),

  getTopMarkets: (limit?: number) =>
    invoke<KalshiMarketSummary[]>('kalshi_get_top_markets', { limit: limit ?? 50 }),

  getDashboardBootstrap: (limit?: number) =>
    invoke<KalshiDashboardBootstrap>('kalshi_get_dashboard_bootstrap', { limit: limit ?? 30 }),

  searchMarkets: (query: string) =>
    invoke<KalshiMarketSummary[]>('kalshi_search_markets', { query }),

  getMarket: (ticker: string) =>
    invoke<KalshiMarketSummary>('kalshi_get_market', { ticker }),

  getCategoryStats: () =>
    invoke<KalshiCategoryStat[]>('kalshi_get_category_stats'),

  getChatContextStatus: () =>
    invoke<KalshiChatContextStatus>('kalshi_get_chat_context_status'),

  refresh: () => invoke<number>('kalshi_refresh'),

  getPredictions: () => invoke<KalshiPrediction[]>('kalshi_get_predictions'),

  gradePending: () => invoke<KalshiGradingSummary>('kalshi_grade_pending_predictions'),

  computeStakeAdjustment: (args: {
    ticker: string;
    category: string;
    contractSide: string;
    recommendedStake: number;
  }) =>
    invoke<StakeAdjustment>('kalshi_compute_stake_adjustment', {
      ticker: args.ticker,
      category: args.category,
      contractSide: args.contractSide,
      recommendedStake: args.recommendedStake,
    }),

  getCalibrationStatus: (rawProbabilityPct: number) =>
    invoke<CalibrationStatus>('kalshi_get_calibration_status', {
      rawProbabilityPct,
    }),

  /** Sidecar agents + edge_engine → forecast ledger row (incl. PASS). */
  analyzeMarketEdge: (ticker: string) =>
    invoke<EdgeAnalysisResult>('kalshi_analyze_market_edge', { ticker }),

  analyzeTopMarketsEdge: (limit?: number, deep?: boolean) =>
    invoke<EdgeAnalysisResult[]>('kalshi_analyze_top_markets_edge', {
      limit: limit ?? 10,
      deep: deep ?? false,
    }),

  resolvePendingForecasts: () => invoke<number>('kalshi_resolve_pending_forecasts'),

  getForecastCalibrationReport: () =>
    invoke<ForecastCalibrationReport>('kalshi_get_forecast_calibration_report'),

  /** Re-fit shrinkage λ from resolved rows (null if fewer than 50 model opinions). Persists λ on success. */
  refitLambda: () => invoke<LambdaFit | null>('kalshi_refit_lambda'),

  getEdgeConfig: () => invoke<EdgeConfig>('kalshi_get_edge_config'),

  setEdgeConfig: (cfg: {
      shrinkage_lambda?: number;
      min_edge?: number;
      fee_multiplier?: number;
      kelly_fraction?: number;
      min_confidence?: number;
    }) =>
      invoke<EdgeConfig>('kalshi_set_edge_config', {
        shrinkageLambda: cfg.shrinkage_lambda ?? NaN,
        minEdge: cfg.min_edge ?? NaN,
        feeMultiplier: cfg.fee_multiplier ?? NaN,
        kellyFraction: cfg.kelly_fraction ?? NaN,
        minConfidence: cfg.min_confidence ?? NaN,
      }),

  evaluateBreakers: () => invoke<BreakerDecision>('kalshi_evaluate_breakers'),

  getLiveOrderEligibility: () =>
    invoke<LiveOrderEligibility>('kalshi_get_live_order_eligibility'),

  guardLiveOrderPath: () => invoke<void>('kalshi_guard_live_order_path'),

  manualReenableBreaker: () =>
    invoke<BreakerDecision['state']>('kalshi_manual_reenable_breaker'),

  getPriceHistory: (ticker: string, limit?: number) =>
    invoke<KalshiPriceHistory>('kalshi_get_price_history', { ticker, limit: limit ?? 200 }),

  recordPaperDecision: (sessionId: string, decision: KalshiTradeDecision) =>
    invoke<PaperRecordResult>('kalshi_record_paper_decision', {
      sessionId,
      decision,
    }),

  /** One-click paper from Edge Board (agent p_final as fair). */
  paperFromEdge: (ticker: string, side: 'YES' | 'NO', stakeDollars?: number, sessionId?: string) =>
    invoke<PaperRecordResult>('kalshi_paper_from_edge', {
      ticker,
      side,
      stakeDollars: stakeDollars ?? null,
      sessionId: sessionId ?? null,
    }),

  getAssetSignal: (ticker: string, horizonDays?: number) =>
    invoke<Record<string, unknown>>('kalshi_get_asset_signal', {
      ticker,
      horizonDays: horizonDays ?? null,
    }),

  syncBankrollToPaperEquity: () =>
    invoke<{ total_bankroll: number }>('paper_sync_bankroll_to_equity'),

  getPaperAnalytics: () => invoke<PaperAnalytics>('paper_get_analytics'),

  getPaperPositions: () => invoke<PaperPosition[]>('paper_get_positions'),

  settlePaperPositions: () =>
    invoke<PaperSettlementSummary>('paper_settle_pending'),

  resetPaperAccount: (startingBalance?: number) =>
    invoke<PaperAccount>('paper_reset_account', { startingBalance: startingBalance ?? null }),
};
