import { invoke } from '@tauri-apps/api/core';
import type {
  KalshiMarket,
  ChatRequest,
  ChatResponse,
  ChatMessage,
  PredictionRecord,
  RedactedConfig,
  DataSourceStatus,
  PropPick,
  PropScoreInput,
  PropScore,
  PropFilterRequest,
} from '../types';

// Kalshi compatibility commands
export const kalshiApi = {
  getMarkets: (category?: string, limit?: number) =>
    invoke<KalshiMarket[]>('kalshi_get_markets', { category, limit }),

  searchMarkets: (query: string) =>
    invoke<KalshiMarket[]>('kalshi_search_markets', { query }),

  getMarket: (ticker: string) =>
    invoke<KalshiMarket>('kalshi_get_market', { ticker }),

  getCategories: () =>
    invoke<string[]>('kalshi_get_categories'),

  getPortfolio: () =>
    invoke<unknown>('kalshi_get_portfolio'),

  getBalance: () =>
    invoke<number>('kalshi_get_balance'),

  getMarketHistory: (ticker: string, startTime?: string, endTime?: string) =>
    invoke<unknown>('kalshi_get_market_history', { ticker, startTime, endTime }),
};

// PrizePicks sports prop commands
export const propsApi = {
  list: (filter?: string, limit?: number) =>
    invoke<PropPick[]>('props_list', { filter, limit }),

  recommend: (limit?: number) =>
    invoke<PropPick[]>('props_recommend', { limit }),

  score: (input: PropScoreInput) =>
    invoke<PropScore>('props_score', { input }),

  filter: (request: PropFilterRequest) =>
    invoke<PropPick[]>('props_filter', { request }),
};

// Chat commands
export const chatApi = {
  send: (request: ChatRequest) =>
    invoke<ChatResponse>('chat_send', { request }),

  getHistory: (sessionId: string) =>
    invoke<ChatMessage[]>('chat_get_history', { sessionId }),

  newSession: () =>
    invoke<string>('chat_new_session'),
};

// Prediction commands
export const predictionApi = {
  add: (input: Omit<PredictionRecord, 'id' | 'result' | 'profit_loss' | 'created_at' | 'resolved_at'>) =>
    invoke<PredictionRecord>('prediction_add', { input }),

  list: (resultFilter?: string, limit?: number) =>
    invoke<PredictionRecord[]>('prediction_list', { resultFilter, limit }),

  grade: (id: string, result: string, profitLoss: number) =>
    invoke<boolean>('prediction_grade', { id, result, profitLoss }),

  exportCsv: (path: string) =>
    invoke<boolean>('prediction_export_csv', { path }),
};

// Bankroll commands
export const bankrollApi = {
  calculateStake: (balance: number, marketProb: number, marketPrice: number, kellyFraction: number) =>
    invoke<number>('bankroll_calculate_stake', { balance, marketProb, marketPrice, kellyFraction }),

  calculateEdge: (estimatedProb: number, marketPrice: number) =>
    invoke<number>('bankroll_calculate_edge', { estimatedProb, marketPrice }),
};

// Market context
export const marketContextApi = {
  detectCategory: (query: string) =>
    invoke<string | null>('market_context_detect_category', { query }),
};

// Notifications
export const notifyApi = {
  sendPick: (title: string, body: string) =>
    invoke<boolean>('notify_send_pick', { title, body }),
};

// Config
export const configApi = {
  get: () =>
    invoke<RedactedConfig>('config_get'),

  update: (newConfig: RedactedConfig) =>
    invoke<boolean>('config_update', { newConfig }),
};

// Data source status
export const statusApi = {
  getDataSourceStatus: () =>
    invoke<DataSourceStatus>('get_data_source_status'),
};
