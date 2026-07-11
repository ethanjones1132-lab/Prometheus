import { useCallback, useEffect, useState } from 'react';
import { kalshiApi } from '../services/kalshi';
import { bankrollApi, configApi, mlApi, notificationApi } from '../services/tauri';
import type {
  ApiStatus,
  AppConfig,
  BankrollConfig,
  BankrollSummary,
  MLModelStatus,
  ModelInfo,
  NotificationSettings,
  SecurityPosture,
} from '../types';

const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  enabled: true,
  game_starting_enabled: true,
  game_final_enabled: true,
  prediction_graded_enabled: true,
  grading_complete_enabled: true,
  kalshi_notifications_enabled: true,
  poll_interval_secs: 60,
  game_starting_minutes_before: 30,
  show_os_notifications: true,
};

const EMPTY_CONFIG: AppConfig = {
  openrouter_api_key: '',
  openrouter_base_url: 'https://openrouter.ai/api/v1',
  llm_provider: 'openrouter',
  opencode_api_key: '',
  selected_model: 'nvidia/nemotron-3-super-120b-a12b:free',
  system_prompt: '',
  max_context_players: 50,
  openweathermap_api_key: '',
  api_sports_key: '',
  brave_api_key: '',
  risk_tolerance: 'moderate',
  preferred_leagues: ['NFL'],
  stat_weighting: 'balanced',
  output_format: 'json_plus_text',
  theme: 'dark',
  kalshi_email: '',
  kalshi_password: '',
  kalshi_poll_interval_secs: 60,
  max_bet_pct: 0.05,
  discord_webhook_url: '',
  telegram_bot_token: '',
  telegram_chat_id: '',
  bot_daily_picks_enabled: true,
  bot_game_alerts_enabled: true,
  bot_grading_results_enabled: true,
  bot_daily_picks_time: '08:00',
};

const LLM_PROVIDERS: Array<{ id: string; label: string; hint: string }> = [
  {
    id: 'openrouter',
    label: 'OpenRouter',
    hint: 'Existing OpenRouter key + model catalog.',
  },
  {
    id: 'opencode_zen',
    label: 'OpenCode Zen',
    hint: 'Pay-per-use gateway — key from https://opencode.ai/auth',
  },
  {
    id: 'opencode_go',
    label: 'OpenCode Go',
    hint: 'Subscription open models — same OpenCode key with Go plan active.',
  },
];

function maskSecret(value: string): string {
  if (!value) return '';
  if (value.length <= 8) return '••••••••';
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
}

export function SettingsView() {
  const [config, setConfig] = useState<AppConfig>(EMPTY_CONFIG);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [opencodeKeyInput, setOpencodeKeyInput] = useState('');
  const [kalshiPasswordInput, setKalshiPasswordInput] = useState('');
  const [weatherKeyInput, setWeatherKeyInput] = useState('');
  const [sportsKeyInput, setSportsKeyInput] = useState('');
  const [braveKeyInput, setBraveKeyInput] = useState('');
  const [discordInput, setDiscordInput] = useState('');
  const [telegramTokenInput, setTelegramTokenInput] = useState('');
  const [leaguesInput, setLeaguesInput] = useState('NFL');
  const [apiStatus, setApiStatus] = useState<ApiStatus | null>(null);
  const [securityPosture, setSecurityPosture] = useState<SecurityPosture | null>(null);
  const [bankrollSummary, setBankrollSummary] = useState<BankrollSummary | null>(null);
  const [bankrollError, setBankrollError] = useState<string | null>(null);
  const [mlStatus, setMlStatus] = useState<MLModelStatus | null>(null);
  const [mlError, setMlError] = useState<string | null>(null);
  const [mlTraining, setMlTraining] = useState(false);
  const [mlTrainMessage, setMlTrainMessage] = useState<string | null>(null);
  const [notificationSettings, setNotificationSettings] =
    useState<NotificationSettings>(DEFAULT_NOTIFICATION_SETTINGS);
  const [edgeLambdaInput, setEdgeLambdaInput] = useState('0.25');
  const [edgeSaving, setEdgeSaving] = useState(false);
  const [edgeMessage, setEdgeMessage] = useState<string | null>(null);
  const [edgeError, setEdgeError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [bankrollLoading, setBankrollLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const cfg = await configApi.get();
      const provider = cfg.llm_provider || 'openrouter';
      const [modelList, posture, notifSettings] = await Promise.all([
        configApi.getAvailableModels(provider),
        configApi.getSecurityPosture().catch(() => null),
        notificationApi.getSettings().catch(() => DEFAULT_NOTIFICATION_SETTINGS),
      ]);
      setConfig({
        ...EMPTY_CONFIG,
        ...cfg,
        llm_provider: provider,
        opencode_api_key: cfg.opencode_api_key ?? '',
      });
      setModels(modelList);
      setSecurityPosture(posture);
      setNotificationSettings(notifSettings);
      setLeaguesInput(cfg.preferred_leagues.join(', '));
      setApiKeyInput('');
      setOpencodeKeyInput('');
      setKalshiPasswordInput('');
      setWeatherKeyInput('');
      setSportsKeyInput('');
      setDiscordInput('');
      setTelegramTokenInput('');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const loadEdgeConfig = useCallback(async () => {
    setEdgeError(null);
    try {
      const cfg = await kalshiApi.getEdgeConfig();
      setEdgeLambdaInput(cfg.shrinkage_lambda.toFixed(3));
    } catch (e) {
      setEdgeError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const handleSaveEdgeLambda = async () => {
    setEdgeSaving(true);
    setEdgeMessage(null);
    setEdgeError(null);
    try {
      const parsed = Number(edgeLambdaInput);
      if (!Number.isFinite(parsed)) {
        throw new Error('Enter a valid number between 0 and 1.');
      }
      const saved = await kalshiApi.setShrinkageLambda(parsed);
      setEdgeLambdaInput(saved.shrinkage_lambda.toFixed(3));
      setEdgeMessage(
        `Shrinkage λ saved (${saved.shrinkage_lambda.toFixed(3)}). Analyze and paper paths use this value.`,
      );
    } catch (e) {
      setEdgeError(e instanceof Error ? e.message : String(e));
    } finally {
      setEdgeSaving(false);
    }
  };

  const reloadModelsForProvider = async (provider: string) => {
    try {
      const modelList = await configApi.getAvailableModels(provider);
      setModels(modelList);
      if (modelList.length > 0 && !modelList.some((m) => m.id === config.selected_model)) {
        setConfig((c) => ({ ...c, selected_model: modelList[0].id, llm_provider: provider }));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const loadBankroll = useCallback(async () => {
    setBankrollLoading(true);
    setBankrollError(null);
    try {
      await bankrollApi.refreshHistoricalBrier().catch(() => 0);
      const cfg: BankrollConfig = await bankrollApi.getConfig();
      const summary = await bankrollApi.getSummary(cfg);
      setBankrollSummary(summary);
    } catch (e) {
      setBankrollError(e instanceof Error ? e.message : String(e));
    } finally {
      setBankrollLoading(false);
    }
  }, []);

  const loadMlStatus = useCallback(async () => {
    setMlError(null);
    try {
      const status = await mlApi.getModelStatus();
      setMlStatus(status);
    } catch (e) {
      setMlError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const handleTrainMl = async () => {
    setMlTraining(true);
    setMlTrainMessage(null);
    setMlError(null);
    try {
      const result = await mlApi.trainModel();
      if (result.status === 'trained') {
        const acc =
          result.cv_accuracy_mean != null
            ? ` — CV ${(result.cv_accuracy_mean * 100).toFixed(1)}%`
            : '';
        setMlTrainMessage(
          `Training complete (${result.samples ?? 0} samples${acc}).`,
        );
      } else {
        setMlTrainMessage(result.message);
      }
      await loadMlStatus();
    } catch (e) {
      setMlError(e instanceof Error ? e.message : String(e));
    } finally {
      setMlTraining(false);
    }
  };

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    void loadBankroll();
    void loadMlStatus();
    void loadEdgeConfig();
  }, [loadBankroll, loadMlStatus, loadEdgeConfig]);

  const handleSave = async () => {
    setSaving(true);
    setMessage(null);
    setError(null);
    try {
      const next: AppConfig = {
        ...config,
        openrouter_api_key: apiKeyInput.trim() || config.openrouter_api_key,
        opencode_api_key: opencodeKeyInput.trim() || config.opencode_api_key,
        llm_provider: config.llm_provider || 'openrouter',
        kalshi_password: kalshiPasswordInput.trim() || config.kalshi_password,
        openweathermap_api_key: weatherKeyInput.trim() || config.openweathermap_api_key,
        api_sports_key: sportsKeyInput.trim() || config.api_sports_key,
        brave_api_key: braveKeyInput.trim() || config.brave_api_key,
        discord_webhook_url: discordInput.trim() || config.discord_webhook_url,
        telegram_bot_token: telegramTokenInput.trim() || config.telegram_bot_token,
        preferred_leagues: leaguesInput
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      await configApi.save(next);
      await notificationApi.saveSettings(notificationSettings);
      setConfig(next);
      setApiKeyInput('');
      setOpencodeKeyInput('');
      setKalshiPasswordInput('');
      setWeatherKeyInput('');
      setSportsKeyInput('');
      setBraveKeyInput('');
      setDiscordInput('');
      setTelegramTokenInput('');
      setMessage('Settings saved. Analyst will use the selected provider on the next message.');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setMessage(null);
    setError(null);
    try {
      const draft: AppConfig = {
        ...config,
        openrouter_api_key: apiKeyInput.trim() || config.openrouter_api_key,
        opencode_api_key: opencodeKeyInput.trim() || config.opencode_api_key,
      };
      await configApi.save(draft);
      setConfig(draft);
      const status = await configApi.checkApiStatus();
      setApiStatus(status);
      const label =
        draft.llm_provider === 'opencode_zen'
          ? 'OpenCode Zen'
          : draft.llm_provider === 'opencode_go'
            ? 'OpenCode Go'
            : 'OpenRouter';
      if (status.connected) {
        setMessage(
          status.model_available
            ? `${label} connected — model available.`
            : `${label} connected — selected model may be unavailable on that gateway.`,
        );
      } else {
        setError(status.error ?? 'Connection failed.');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  const provider = config.llm_provider || 'openrouter';
  const isOpenCode = provider === 'opencode_zen' || provider === 'opencode_go';

  if (loading) {
    return (
      <section className="page">
        <div className="card">
          <div className="state">Loading settings…</div>
        </div>
      </section>
    );
  }

  return (
    <section className="page settingsPage">
      <header className="kalshiHeader">
        <div>
          <h2>Settings</h2>
          <p className="muted">
            LLM provider (OpenRouter / OpenCode Zen / OpenCode Go), model selection for Analyst, risk controls, and
            notification hooks. Secrets stay masked here and are redacted from diagnostics.
          </p>
        </div>
        <div className="panelToolbar">
          <button type="button" className="ghostBtn" onClick={() => void load()}>
            Reload
          </button>
          <button type="button" className="primaryBtn" disabled={saving} onClick={() => void handleSave()}>
            {saving ? 'Saving…' : 'Save settings'}
          </button>
        </div>
      </header>

      {message && <div className="banner success">{message}</div>}
      {error && <div className="banner error">{error}</div>}

      {securityPosture && (
        <div className="card settingsWide securityPosture">
          <div className="paperPortfolioHeader">
            <h3>Security posture</h3>
            <span className={`statusPill ${securityPosture.csp_enforced ? 'ok' : 'bad'}`}>
              {securityPosture.csp_enforced ? 'CSP enforced' : 'CSP missing'}
            </span>
          </div>
          <div className="metricGrid">
            <div className="metricCard">
              <span>Diagnostics</span>
              <strong>{securityPosture.secrets_redacted ? 'Secrets redacted from diagnostics' : 'Redaction missing'}</strong>
            </div>
            <div className="metricCard">
              <span>Secret store</span>
              <strong>{securityPosture.secret_store}</strong>
            </div>
            <div className="metricCard">
              <span>Protected fields</span>
              <strong>{securityPosture.redacted_fields.length}</strong>
              <small>{securityPosture.redacted_fields.join(', ') || 'No secrets configured'}</small>
            </div>
          </div>
          {securityPosture.warnings.map((warning) => (
            <p key={warning} className="warnText">{warning}</p>
          ))}
        </div>
      )}

      <div className="card settingsWide">
        <h3>Bankroll & cap sync</h3>
        {bankrollLoading ? (
          <div className="state">Loading bankroll sync…</div>
        ) : bankrollError ? (
          <div className="banner error">{bankrollError}</div>
        ) : bankrollSummary ? (
          <>
            <div className="metricGrid">
              <div className="metricCard">
                <span>Current bankroll</span>
                <strong>${bankrollSummary.current_bankroll.toFixed(2)}</strong>
              </div>
              <div className="metricCard">
                <span>Daily cap</span>
                <strong>${bankrollSummary.daily_limit_used.toFixed(2)} / ${bankrollSummary.config.daily_bet_limit.toFixed(2)}</strong>
                <small>${bankrollSummary.remaining_daily.toFixed(2)} remaining</small>
              </div>
              <div className="metricCard">
                <span>Weekly cap</span>
                <strong>${bankrollSummary.weekly_limit_used.toFixed(2)} / ${bankrollSummary.config.weekly_bet_limit.toFixed(2)}</strong>
                <small>${bankrollSummary.remaining_weekly.toFixed(2)} remaining</small>
              </div>
              <div className="metricCard">
                <span>Open exposure</span>
                <strong>${(bankrollSummary.prediction_open_exposure + bankrollSummary.paper_open_exposure).toFixed(2)}</strong>
                <small>Predictions ${bankrollSummary.prediction_open_exposure.toFixed(2)} · Paper ${bankrollSummary.paper_open_exposure.toFixed(2)}</small>
              </div>
              <div className="metricCard">
                <span>Local max stake</span>
                <strong>${(bankrollSummary.config.total_bankroll * config.max_bet_pct).toFixed(2)}</strong>
                <small>{(config.max_bet_pct * 100).toFixed(1)}% of bankroll</small>
              </div>
            </div>
            <div className="formGrid">
              <label>
                Local max bet % of bankroll
                <input
                  type="number"
                  min={0.1}
                  max={25}
                  step={0.1}
                  value={(config.max_bet_pct * 100).toFixed(1)}
                  onChange={(e) =>
                    setConfig({ ...config, max_bet_pct: Number(e.target.value) / 100 })
                  }
                />
              </label>
            </div>
            <p className="muted">
              Synced from <code>predictions.db</code> and paper positions at {bankrollSummary.synced_at}.
            </p>
          </>
        ) : null}
      </div>

      <div className="card settingsWide">
        <h3>Edge engine (shrinkage λ)</h3>
        <p className="muted" style={{ marginTop: 0 }}>
          Manual override for plan §4.1 shrinkage between model and market probabilities. Calibration
          re-fit also writes here; analyze and paper decisions load the persisted value from{' '}
          <code>edge_config</code>.
        </p>
        {edgeError ? <div className="banner error">{edgeError}</div> : null}
        {edgeMessage ? <div className="banner success">{edgeMessage}</div> : null}
        <div className="formGrid">
          <label>
            Shrinkage λ (0–1)
            <input
              type="number"
              min={0}
              max={1}
              step={0.001}
              value={edgeLambdaInput}
              onChange={(e) => setEdgeLambdaInput(e.target.value)}
            />
          </label>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem', marginTop: '0.75rem', flexWrap: 'wrap' }}>
          <button
            type="button"
            className="primaryBtn"
            disabled={edgeSaving}
            onClick={() => void handleSaveEdgeLambda()}
          >
            {edgeSaving ? 'Saving…' : 'Save shrinkage λ'}
          </button>
          <button type="button" className="ghostBtn" disabled={edgeSaving} onClick={() => void loadEdgeConfig()}>
            Reload from DB
          </button>
        </div>
      </div>

      <div className="card settingsWide">
        <h3>ML multi-category readiness</h3>
        <p className="muted" style={{ marginTop: 0 }}>
          Unified + per-category sidecars retrain automatically when the Kalshi auto-grader
          resolves markets and you have at least 10 total graded rows. Use manual train to refresh on
          demand.
        </p>
        <div style={{ display: 'flex', gap: '0.5rem', marginBottom: '0.75rem', flexWrap: 'wrap' }}>
          <button
            type="button"
            className="primaryBtn"
            disabled={mlTraining}
            onClick={() => void handleTrainMl()}
          >
            {mlTraining ? 'Training…' : 'Train unified + sidecar models'}
          </button>
          <button
            type="button"
            className="ghostBtn"
            disabled={mlTraining}
            onClick={() => void loadMlStatus()}
          >
            Refresh status
          </button>
        </div>
        {mlTrainMessage ? <p className="muted">{mlTrainMessage}</p> : null}
        {mlError ? (
          <div className="banner error">{mlError}</div>
        ) : mlStatus ? (
          <>
            <p className="muted">{mlStatus.message}</p>
            <p className="muted">
              Phase 3 progress:{' '}
              <strong>
                {mlStatus.trainable_non_sports_categories ?? 0}/
                {mlStatus.non_sports_sidecar_target ?? 3}
              </strong>{' '}
              non-sports categories ready for sidecars (Politics, Economics, Weather).
              {mlStatus.phase_3_data_metric_ready ? (
                <>
                  {' '}
                  <strong>ROADMAP data metric met.</strong>
                </>
              ) : null}
            </p>
            <p className="muted">
              Kalshi paper journal:{' '}
              <strong>{mlStatus.kalshi_resolved_predictions ?? 0}</strong> resolved ·{' '}
              <strong>{mlStatus.kalshi_pending_predictions ?? 0}</strong> pending (ticker in
              decision JSON; totals above include all products in <code>predictions.db</code>).
            </p>
            {mlStatus.next_sidecar_category != null &&
            mlStatus.next_sidecar_samples_needed != null ? (
              <p className="muted">
                Next sidecar unlock:{' '}
                <strong>{mlStatus.next_sidecar_category}</strong> needs{' '}
                <strong>{mlStatus.next_sidecar_samples_needed}</strong> more graded
                prediction
                {mlStatus.next_sidecar_samples_needed === 1 ? '' : 's'}.
              </p>
            ) : null}
            {mlStatus.auto_retrain_eligible === false &&
            (mlStatus.resolved_until_auto_retrain ?? 0) > 0 ? (
              <p className="muted">
                Auto-retrain after grading unlocks at 10 total resolved predictions —{' '}
                <strong>{mlStatus.resolved_until_auto_retrain}</strong> more needed.
              </p>
            ) : mlStatus.auto_retrain_eligible ? (
              <p className="muted">
                Auto-retrain after grading: <strong>active</strong> (≥10 resolved predictions).
              </p>
            ) : null}
            <div className="metricGrid">
              <div className="metricCard">
                <span>Unified model</span>
                <strong>{mlStatus.model_exists ? 'Trained' : 'Not trained'}</strong>
                <small>
                  {mlStatus.resolved_predictions} resolved · {mlStatus.pending_predictions} pending
                  {mlStatus.trained_at
                    ? ` · last trained ${new Date(mlStatus.trained_at).toLocaleString()}`
                    : ''}
                </small>
              </div>
              {mlStatus.samples != null && mlStatus.cv_accuracy_mean != null ? (
                <div className="metricCard">
                  <span>CV accuracy</span>
                  <strong>
                    {(mlStatus.cv_accuracy_mean * 100).toFixed(1)}%
                    {mlStatus.cv_accuracy_std != null
                      ? ` ± ${(mlStatus.cv_accuracy_std * 100).toFixed(1)}%`
                      : ''}
                  </strong>
                  <small>{mlStatus.samples} training samples</small>
                </div>
              ) : null}
            </div>
            {mlStatus.category_stats.length > 0 ? (
              <>
                <p className="muted" style={{ marginTop: '0.75rem', marginBottom: '0.25rem' }}>
                  Per-category graded counts (Kalshi paper rows with market ticker only):
                </p>
                <ul className="muted">
                {mlStatus.category_stats.map((s) => (
                  <li key={s.category}>
                    {s.category}: {s.resolved_count}/{s.min_resolved_for_sidecar} graded,{' '}
                    {s.pending_count} pending —{' '}
                    {s.trainable
                      ? 'ready for sidecar model'
                      : `${s.samples_until_trainable} more graded needed for sidecar`}
                  </li>
                ))}
              </ul>
              </>
            ) : (
              <p className="muted">No categorized predictions in DB yet.</p>
            )}
            {mlStatus.training_category_breakdown &&
            Object.keys(mlStatus.training_category_breakdown).length > 0 ? (
              <p className="muted">
                Last training mix:{' '}
                {Object.entries(mlStatus.training_category_breakdown)
                  .map(([cat, n]) => `${cat} (${n})`)
                  .join(' · ')}
              </p>
            ) : null}
            {mlStatus.per_category_models && Object.keys(mlStatus.per_category_models).length > 0 ? (
              <p className="muted">
                Active sidecars:{' '}
                {Object.entries(mlStatus.per_category_models)
                  .map(([name, m]) => {
                    const cv =
                      m.cv_accuracy_mean != null
                        ? `, CV ${(m.cv_accuracy_mean * 100).toFixed(1)}%`
                        : '';
                    return m.model_exists
                      ? `${name} (${m.samples} samples${cv})`
                      : `${name} (missing file)`;
                  })
                  .join(' · ')}
              </p>
            ) : null}
          </>
        ) : (
          <div className="state">Loading ML status…</div>
        )}
      </div>

      <div className="settingsGrid">
        <div className="card">
          <h3>Analyst LLM</h3>
          <p className="muted" style={{ marginTop: 0 }}>
            Choose who serves Analyst chat. OpenCode Zen and Go share one API key from{' '}
            <a href="https://opencode.ai/auth" target="_blank" rel="noreferrer">
              opencode.ai/auth
            </a>
            . Go requires an active Go subscription; Zen is pay-per-use (plus free models).
          </p>
          <div className="formGrid">
            <label>
              Provider
              <select
                value={provider}
                onChange={(e) => {
                  const next = e.target.value;
                  setConfig({ ...config, llm_provider: next });
                  void reloadModelsForProvider(next);
                }}
              >
                {LLM_PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </select>
            </label>
            <p className="muted" style={{ gridColumn: '1 / -1', margin: 0 }}>
              {LLM_PROVIDERS.find((p) => p.id === provider)?.hint}
            </p>

            {provider === 'openrouter' && (
              <>
                <label>
                  OpenRouter API key
                  <input
                    type="password"
                    placeholder={
                      config.openrouter_api_key ? maskSecret(config.openrouter_api_key) : 'sk-or-v1-…'
                    }
                    value={apiKeyInput}
                    onChange={(e) => setApiKeyInput(e.target.value)}
                    autoComplete="off"
                  />
                </label>
                <label>
                  OpenRouter base URL
                  <input
                    value={config.openrouter_base_url}
                    onChange={(e) => setConfig({ ...config, openrouter_base_url: e.target.value })}
                  />
                </label>
              </>
            )}

            {isOpenCode && (
              <label style={{ gridColumn: '1 / -1' }}>
                OpenCode API key (Zen / Go)
                <input
                  type="password"
                  placeholder={
                    config.opencode_api_key
                      ? maskSecret(config.opencode_api_key)
                      : 'Paste key from opencode.ai/auth'
                  }
                  value={opencodeKeyInput}
                  onChange={(e) => setOpencodeKeyInput(e.target.value)}
                  autoComplete="off"
                />
              </label>
            )}

            <label style={{ gridColumn: '1 / -1' }}>
              Model (used by Analyst)
              <select
                value={
                  models.some((m) => m.id === config.selected_model)
                    ? config.selected_model
                    : models[0]?.id ?? config.selected_model
                }
                onChange={(e) => setConfig({ ...config, selected_model: e.target.value })}
              >
                {models.map((m) => (
                  <option key={`${m.provider}-${m.id}`} value={m.id}>
                    {m.name} — {m.cost}
                    {m.description ? ` · ${m.description.slice(0, 60)}` : ''}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Max context players
              <input
                type="number"
                min={10}
                max={200}
                value={config.max_context_players}
                onChange={(e) =>
                  setConfig({ ...config, max_context_players: Number(e.target.value) })
                }
              />
            </label>
          </div>
          <div className="settingsActions">
            <button
              type="button"
              className="ghostBtn"
              disabled={testing}
              onClick={() => void handleTestConnection()}
            >
              {testing ? 'Testing…' : 'Test connection'}
            </button>
            {apiStatus && (
              <span className={`statusPill ${apiStatus.connected ? 'ok' : 'bad'}`}>
                {apiStatus.connected ? 'Connected' : 'Disconnected'}
                {apiStatus.credits_remaining ? ` · ${apiStatus.credits_remaining}` : ''}
              </span>
            )}
          </div>
        </div>

        <div className="card">
          <h3>Analysis preferences</h3>
          <div className="formGrid">
            <label>
              Risk tolerance
              <select
                value={config.risk_tolerance}
                onChange={(e) => setConfig({ ...config, risk_tolerance: e.target.value })}
              >
                <option value="conservative">Conservative</option>
                <option value="moderate">Moderate</option>
                <option value="aggressive">Aggressive</option>
              </select>
            </label>
            <label>
              Stat weighting
              <select
                value={config.stat_weighting}
                onChange={(e) => setConfig({ ...config, stat_weighting: e.target.value })}
              >
                <option value="season_avg">Season average</option>
                <option value="last3">Last 3 games</option>
                <option value="matchup_adjusted">Matchup adjusted</option>
                <option value="balanced">Balanced</option>
              </select>
            </label>
            <label>
              Output format
              <select
                value={config.output_format}
                onChange={(e) => setConfig({ ...config, output_format: e.target.value })}
              >
                <option value="json_first">JSON first</option>
                <option value="text_only">Text only</option>
                <option value="json_plus_text">JSON + text</option>
              </select>
            </label>
            <label>
              Preferred leagues
              <input
                value={leaguesInput}
                onChange={(e) => setLeaguesInput(e.target.value)}
                placeholder="NFL, NBA, MLB"
              />
            </label>
            <label>
              Theme
              <select
                value={config.theme}
                onChange={(e) => setConfig({ ...config, theme: e.target.value })}
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </label>
          </div>
        </div>

        <div className="card">
          <h3>Kalshi & data keys</h3>
          <div className="formGrid">
            <label>
              Kalshi email
              <input
                value={config.kalshi_email}
                onChange={(e) => setConfig({ ...config, kalshi_email: e.target.value })}
              />
            </label>
            <label>
              Kalshi password
              <input
                type="password"
                placeholder={config.kalshi_password ? maskSecret(config.kalshi_password) : 'Optional'}
                value={kalshiPasswordInput}
                onChange={(e) => setKalshiPasswordInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              Poll interval (seconds)
              <input
                type="number"
                min={15}
                max={600}
                value={config.kalshi_poll_interval_secs}
                onChange={(e) =>
                  setConfig({ ...config, kalshi_poll_interval_secs: Number(e.target.value) })
                }
              />
            </label>
            <label>
              OpenWeatherMap key
              <input
                type="password"
                placeholder={config.openweathermap_api_key ? 'Set' : 'Optional'}
                value={weatherKeyInput}
                onChange={(e) => setWeatherKeyInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              API-Sports key
              <input
                type="password"
                placeholder={config.api_sports_key ? 'Set' : 'Optional'}
                value={sportsKeyInput}
                onChange={(e) => setSportsKeyInput(e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              Brave Search API key
              <input
                type="password"
                placeholder={
                  config.brave_api_key
                    ? `Set (${maskSecret(config.brave_api_key)})`
                    : 'Optional — Analyst web evidence'
                }
                value={braveKeyInput}
                onChange={(e) => setBraveKeyInput(e.target.value)}
                autoComplete="off"
              />
              <span className="fieldHint">
                From{' '}
                <a
                  href="https://api-dashboard.search.brave.com/"
                  target="_blank"
                  rel="noreferrer"
                >
                  api-dashboard.search.brave.com
                </a>
                . Used first for Analyst web grounding; falls back to DuckDuckGo if empty.
              </span>
            </label>
          </div>
        </div>

        <div className="card">
          <h3>Notifications & bot</h3>
          <div className="formGrid">
            <label>
              Discord webhook
              <input
                type="password"
                placeholder={config.discord_webhook_url ? maskSecret(config.discord_webhook_url) : 'https://discord.com/api/webhooks/…'}
                value={discordInput}
                onChange={(e) => setDiscordInput(e.target.value)}
              />
            </label>
            <label>
              Telegram bot token
              <input
                type="password"
                placeholder={config.telegram_bot_token ? 'Set' : 'Optional'}
                value={telegramTokenInput}
                onChange={(e) => setTelegramTokenInput(e.target.value)}
              />
            </label>
            <label>
              Telegram chat ID
              <input
                value={config.telegram_chat_id}
                onChange={(e) => setConfig({ ...config, telegram_chat_id: e.target.value })}
              />
            </label>
            <label>
              Daily picks time
              <input
                value={config.bot_daily_picks_time}
                onChange={(e) => setConfig({ ...config, bot_daily_picks_time: e.target.value })}
                placeholder="08:00"
              />
            </label>
          </div>
          <div className="toggleRow">
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_daily_picks_enabled}
                onChange={(e) => setConfig({ ...config, bot_daily_picks_enabled: e.target.checked })}
              />
              Daily picks
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_game_alerts_enabled}
                onChange={(e) => setConfig({ ...config, bot_game_alerts_enabled: e.target.checked })}
              />
              Game alerts
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={config.bot_grading_results_enabled}
                onChange={(e) => setConfig({ ...config, bot_grading_results_enabled: e.target.checked })}
              />
              Grading results
            </label>
            <label className="toggleLabel">
              <input
                type="checkbox"
                checked={notificationSettings.kalshi_notifications_enabled}
                onChange={(e) =>
                  setNotificationSettings({
                    ...notificationSettings,
                    kalshi_notifications_enabled: e.target.checked,
                  })
                }
              />
              Kalshi market resolved alerts
            </label>
          </div>
        </div>

        <div className="card settingsWide">
          <h3>System prompt</h3>
          <p className="muted">Override the default Kalshi Monster analyst persona. Leave blank to reload the built-in prompt on next app start.</p>
          <textarea
            className="promptArea"
            rows={12}
            value={config.system_prompt}
            onChange={(e) => setConfig({ ...config, system_prompt: e.target.value })}
          />
        </div>
      </div>
    </section>
  );
}
