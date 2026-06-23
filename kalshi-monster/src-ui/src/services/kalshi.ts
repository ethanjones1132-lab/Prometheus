import { invoke } from '@tauri-apps/api/core';
import type {
  KalshiCategoryStat,
  KalshiDashboardBootstrap,
  KalshiMarketSummary,
  KalshiPrediction,
  KalshiPriceHistory,
  KalshiTradeDecision,
  CalibrationStatus,
  PaperAccount,
  PaperAnalytics,
  PaperPosition,
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

  getPriceHistory: (ticker: string, limit?: number) =>
    invoke<KalshiPriceHistory>('kalshi_get_price_history', { ticker, limit: limit ?? 200 }),

  recordPaperDecision: (sessionId: string, decision: KalshiTradeDecision) =>
    invoke<string>('kalshi_record_paper_decision', { sessionId, decision }),

  getPaperAnalytics: () => invoke<PaperAnalytics>('paper_get_analytics'),

  getPaperPositions: () => invoke<PaperPosition[]>('paper_get_positions'),

  settlePaperPositions: () =>
    invoke<PaperSettlementSummary>('paper_settle_pending'),

  resetPaperAccount: (startingBalance?: number) =>
    invoke<PaperAccount>('paper_reset_account', { startingBalance: startingBalance ?? null }),
};
