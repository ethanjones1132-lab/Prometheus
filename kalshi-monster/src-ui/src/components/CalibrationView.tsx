import { useCallback, useEffect, useState } from 'react';
import { finceptApi } from '../services/tauri';
import { kalshiApi } from '../services/kalshi';
import type { EdgeAnalysisResult, ForecastCalibrationReport } from '../types/kalshi';

function pct(p: number | null | undefined): string {
  if (p == null || !Number.isFinite(p)) return '—';
  return `${(p * 100).toFixed(1)}%`;
}

function brier(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return '—';
  return v.toFixed(4);
}

function money(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return '—';
  const sign = v > 0 ? '+' : '';
  return `${sign}$${v.toFixed(2)}`;
}

export function CalibrationView() {
  const [report, setReport] = useState<ForecastCalibrationReport | null>(null);
  const [bridge, setBridge] = useState<{ online: boolean; degraded: boolean; last_error?: string | null } | null>(
    null,
  );
  const [recent, setRecent] = useState<EdgeAnalysisResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [rep, st] = await Promise.all([
        kalshiApi.getForecastCalibrationReport(),
        finceptApi.getBridgeStatus().catch(() => null),
      ]);
      setReport(rep);
      if (st) setBridge(st);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const resolvePending = async () => {
    setActionBusy('resolve');
    setMessage(null);
    setError(null);
    try {
      const n = await kalshiApi.resolvePendingForecasts();
      setMessage(
        n === 0
          ? 'No pending forecasts resolved — markets still open or none in the ledger.'
          : `Resolved ${n} forecast row(s) from live Kalshi settlement results.`,
      );
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActionBusy(null);
    }
  };

  const analyzeTop = async (limit: number) => {
    setActionBusy('analyze');
    setMessage(null);
    setError(null);
    try {
      const rows = await kalshiApi.analyzeTopMarketsEdge(limit);
      setRecent(rows);
      setMessage(
        rows.length === 0
          ? 'No markets analyzed (empty tape or all failed). Check Command desk for markets + Fincept bridge.'
          : `Logged ${rows.length} forecast row(s) via edge engine (PASS included). No fabricated outcomes.`,
      );
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActionBusy(null);
    }
  };

  const gateOk = report?.gate_passed === true;
  const progress =
    report != null ? Math.min(100, (report.resolved_count / 200) * 100) : 0;

  return (
    <section className="page kalshiPage" aria-label="Calibration surface">
      <header className="kalshiHeader">
        <div>
          <h2>Calibration</h2>
          <p className="muted">
            Forecast ledger, Brier scores, and the live-trading gate. Evidence only — never synthetic
            outcomes. Live orders stay locked until ≥200 resolved, Brier(p_final) ≤ Brier(p_market), and
            paper P&amp;L &gt; 0.
          </p>
        </div>
        <button type="button" className="primaryButton" onClick={() => void refresh()} disabled={loading}>
          {loading ? 'Refreshing…' : 'Refresh report'}
        </button>
      </header>

      {bridge && (
        <div className={`insightCard ${bridge.online ? 'accent' : ''}`}>
          <span>Fincept agents</span>
          <strong>{bridge.online ? 'Online' : bridge.degraded ? 'Degraded' : 'Offline'}</strong>
          <p>
            {bridge.online
              ? 'Analyze actions call technical + contract_tape agents, then Rust aggregate/evaluate.'
              : 'Sidecar offline — analyze still logs market-only rows (p_model null, p_final = p_market).'}
            {bridge.last_error ? ` — ${bridge.last_error}` : ''}
          </p>
        </div>
      )}

      {error && (
        <p className="errorText" role="alert">
          {error}
        </p>
      )}
      {message && <p className="muted">{message}</p>}

      <div className="mechanicsGrid" style={{ marginBottom: '1rem' }}>
        <div>
          <span>Gate</span>
          <strong className={gateOk ? 'pos' : 'neg'}>{gateOk ? 'OPEN' : 'LOCKED'}</strong>
        </div>
        <div>
          <span>Resolved</span>
          <strong>
            {report?.resolved_count ?? '—'} / 200
          </strong>
        </div>
        <div>
          <span>Unresolved</span>
          <strong>{report?.unresolved_count ?? '—'}</strong>
        </div>
        <div>
          <span>Paper P&amp;L</span>
          <strong className={(report?.paper_pnl ?? 0) > 0 ? 'pos' : 'neg'}>
            {money(report?.paper_pnl)}
          </strong>
        </div>
      </div>

      <div
        className="insightCard"
        style={{ marginBottom: '1rem' }}
        aria-label="Resolved forecast progress"
      >
        <span>Progress to gate sample size</span>
        <strong>{progress.toFixed(0)}%</strong>
        <div
          style={{
            marginTop: '0.5rem',
            height: 8,
            borderRadius: 4,
            background: 'var(--border, #333)',
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              width: `${progress}%`,
              height: '100%',
              background: gateOk ? 'var(--ok, #3d9a5f)' : 'var(--accent, #4a7dff)',
            }}
          />
        </div>
      </div>

      <section className="modalSection">
        <h4>Brier summary (resolved rows only)</h4>
        <div className="mechanicsGrid">
          <div>
            <span>Brier(p_market)</span>
            <strong>{brier(report?.brier_market)}</strong>
          </div>
          <div>
            <span>Brier(p_final)</span>
            <strong>{brier(report?.brier_final)}</strong>
          </div>
          <div>
            <span>Brier(p_model)</span>
            <strong>
              {brier(report?.brier_model)}
              {report && report.n_model > 0 ? ` (n=${report.n_model})` : ''}
            </strong>
          </div>
          <div>
            <span>Market on model rows</span>
            <strong>{brier(report?.brier_market_on_model_rows)}</strong>
          </div>
        </div>
        {report?.gate_reasons && report.gate_reasons.length > 0 && (
          <ul className="muted" style={{ marginTop: '0.75rem', paddingLeft: '1.2rem' }}>
            {report.gate_reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        )}
      </section>

      <section className="modalSection">
        <h4>Actions</h4>
        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          <button
            type="button"
            className="primaryButton"
            disabled={actionBusy != null}
            onClick={() => void analyzeTop(10)}
          >
            {actionBusy === 'analyze' ? 'Analyzing…' : 'Analyze top 10 (log forecasts)'}
          </button>
          <button
            type="button"
            className="ghostBtn"
            disabled={actionBusy != null}
            onClick={() => void analyzeTop(5)}
          >
            Analyze top 5
          </button>
          <button
            type="button"
            className="ghostBtn"
            disabled={actionBusy != null}
            onClick={() => void resolvePending()}
          >
            {actionBusy === 'resolve' ? 'Resolving…' : 'Resolve settled forecasts'}
          </button>
        </div>
        <p className="muted" style={{ marginTop: '0.5rem' }}>
          Analyze writes pending ledger rows from live Kalshi quotes + agents. Resolve only applies when
          Kalshi has a Yes/No result — never invented.
        </p>
      </section>

      {recent.length > 0 && (
        <section className="modalSection">
          <h4>Last analyze batch</h4>
          <div className="tableWrap">
            <table className="dataTable">
              <thead>
                <tr>
                  <th>Ticker</th>
                  <th>p_market</th>
                  <th>p_model</th>
                  <th>p_final</th>
                  <th>Verdict</th>
                  <th>Agents</th>
                </tr>
              </thead>
              <tbody>
                {recent.map((r) => (
                  <tr key={r.forecast_id}>
                    <td>
                      <code>{r.market_ticker}</code>
                    </td>
                    <td>{pct(r.p_market)}</td>
                    <td>{r.p_model == null ? '—' : pct(r.p_model)}</td>
                    <td>{pct(r.p_final)}</td>
                    <td>{r.verdict}</td>
                    <td>
                      {r.signals_opining}/{r.signals_received}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </section>
  );
}
