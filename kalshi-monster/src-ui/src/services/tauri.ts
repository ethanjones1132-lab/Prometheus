import { invoke } from '@tauri-apps/api/core';
import type {
  AnalysisContext,
  ApiStatus,
  AppConfig,
  BankrollConfig,
  BankrollSummary,
  ChatMessage,
  ChatSession,
  DataSourceStatus,
  EdgeAnalysisInput,
  ModelInfo,
  OpenRouterResponse,
  PredictionRecord,
  PropAnalysisResult,
  SecurityPosture,
  ScoredProp,
} from '../types';

// ── Config ──────────────────────────────────────────────────────

export const configApi = {
  get: () => invoke<AppConfig>('get_config'),

  save: (config: AppConfig) => invoke<void>('save_config', { config }),

  checkApiStatus: () => invoke<ApiStatus>('check_api_status'),

  getSecurityPosture: () => invoke<SecurityPosture>('get_security_posture'),

  getAvailableModels: () => invoke<ModelInfo[]>('get_available_models'),
};

// ── Chat ────────────────────────────────────────────────────────

export const chatApi = {
  newSession: (name?: string) => invoke<ChatSession>('new_chat_session', { name: name ?? null }),

  sendMessage: (message: string, sessionId: string) =>
    invoke<OpenRouterResponse>('send_message', { message, sessionId }),

  sendMessageStream: (message: string, sessionId: string) =>
    invoke<void>('send_message_stream', { message, sessionId }),

  listSessions: () => invoke<ChatSession[]>('list_chat_sessions'),

  deleteSession: (sessionId: string) => invoke<void>('delete_chat_session', { sessionId }),

  getHistory: (sessionId: string) =>
    invoke<ChatMessage[]>('get_session_messages', { sessionId }),
};

// ── Analysis / props ────────────────────────────────────────────

export const analysisApi = {
  analyzeProp: (input: EdgeAnalysisInput) =>
    invoke<PropAnalysisResult>('analyze_prop', { input }),

  analyzeMultiple: (inputs: EdgeAnalysisInput[]) =>
    invoke<AnalysisContext>('analyze_multiple_props', { inputs }),

  getScoredByTier: (inputs: EdgeAnalysisInput[], minTier: string) =>
    invoke<ScoredProp[]>('get_scored_props_by_tier', { inputs, minTier }),
};

// ── Predictions ─────────────────────────────────────────────────

export const predictionApi = {
  getSession: (sessionId: string) =>
    invoke<PredictionRecord[]>('get_session_predictions', { sessionId }),

  getAll: () => invoke<PredictionRecord[]>('get_all_predictions'),

  gradePending: () => invoke<unknown>('grade_pending_predictions'),

  exportCsv: () => invoke<string>('export_predictions_csv'),
};

// ── Bankroll ────────────────────────────────────────────────────

export const bankrollApi = {
  getConfig: () => invoke<BankrollConfig>('get_bankroll_config'),

  saveConfig: (config: BankrollConfig) =>
    invoke<void>('save_bankroll_config', { config }),

  getSummary: (config: BankrollConfig) =>
    invoke<BankrollSummary>('get_bankroll_summary', { config }),

  refreshHistoricalBrier: () =>
    invoke<number>('refresh_historical_brier'),
};

// ── Status ──────────────────────────────────────────────────────

export const statusApi = {
  getDataSourceStatus: () => invoke<DataSourceStatus>('get_data_source_status'),
};
