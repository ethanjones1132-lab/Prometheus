import { invoke } from '@tauri-apps/api/core';
import type {
  MLTrainingResult,
  AnalysisContext,
  ApiStatus,
  AppConfig,
  AppSecrets,
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
  SecretKey,
  SecurityPosture,
  ScoredProp,
  MLModelStatus,
  NotificationSettings,
} from '../types';

// ── Config ──────────────────────────────────────────────────────

export const configApi = {
  get: () => invoke<AppConfig>('get_config'),

  save: (config: AppConfig) => invoke<void>('save_config', { config }),

  checkApiStatus: () => invoke<ApiStatus>('check_api_status'),

  getSecurityPosture: () => invoke<SecurityPosture>('get_security_posture'),

  getAvailableModels: (provider?: string | null) =>
    invoke<ModelInfo[]>('get_available_models', { provider: provider ?? null }),

  getSecrets: () => invoke<AppSecrets>('get_secrets'),

  saveSecret: (key: SecretKey, value: string) =>
    invoke<void>('save_secret', { key, value }),

  deleteSecret: (key: SecretKey) => invoke<void>('delete_secret', { key }),
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

  renameSession: (sessionId: string, newName: string) =>
    invoke<ChatSession>('rename_chat_session', { sessionId, newName }),

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

// ── ML (multi-category readiness) ───────────────────────────────

export const mlApi = {
  getModelStatus: () => invoke<MLModelStatus>('ml_get_model_status', { modelPath: null }),

  trainModel: () =>
    invoke<MLTrainingResult>('ml_train_model', { dbPath: null, outputPath: null }),
};

// ── In-app notifications ────────────────────────────────────────

export const notificationApi = {
  getSettings: () => invoke<NotificationSettings>('get_notification_settings'),

  saveSettings: (settings: NotificationSettings) =>
    invoke<void>('save_notification_settings', { settings }),
};

// ── Fincept sidecar (Phase 1 world markets) ─────────────────────

export type FinceptBridgeStatus = {
  online: boolean;
  degraded: boolean;
  base_url?: string | null;
  last_error?: string | null;
  restarts_remaining: number;
  /** Sprint 3.2 — agent ops diagnostics */
  last_agent_latency_ms?: number | null;
  agent_calls?: number;
  agent_calls_opining?: number;
  signals_received_total?: number;
  signals_opining_total?: number;
  last_agent_call_at?: string | null;
  /** Fraction of agent calls with ≥1 non-null probability */
  opining_rate?: number;
};

export const finceptApi = {
  getBridgeStatus: () => invoke<FinceptBridgeStatus>('get_fincept_bridge_status'),

  startDev: () => invoke<FinceptBridgeStatus>('fincept_bridge_start_dev'),

  stop: () => invoke<FinceptBridgeStatus>('fincept_bridge_stop'),

  getMarketTracker: (category?: string | null) =>
    invoke<Record<string, unknown>>('get_fincept_market_tracker', { category: category ?? null }),
};
