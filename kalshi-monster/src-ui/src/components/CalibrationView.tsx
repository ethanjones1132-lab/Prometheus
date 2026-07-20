import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import { finceptApi } from '../services/tauri';
import { kalshiApi } from '../services/kalshi';
import type { EdgeAnalysisResult, ForecastCalibrationReport, BreakerDecision, LambdaFit, EdgeConfig } from '../types/kalshi';
import { ReliabilityDiagram } from './ReliabilityDiagram';
import { LiveDot } from './brand/LiveDot';
import { notifyPaperUpdated } from '../utils/paperEvents';
import { formatFeePreviewLine } from '../utils/kalshiFees';

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

function absEdge(r: EdgeAnalysisResult): number {
  return Math.max(Math.abs(r.edge_net_yes), Math.abs(r.edge_net_no));
}

function agentSignalRows(breakdown: unknown): Array<{
  agent: string;
  probability: number | null;
  confidence: number;
  rationale?: string;
}> {
  if (!breakdown || typeof breakdown !== 'object') return [];
  const signals = (breakdown as { signals?: unknown }).signals;
  if (!Array.isArray(signals)) return [];
  return signals.map((s) => {
    const row = s as Record<string, unknown>;
    return {
      agent: String(row.agent ?? '?'),
      probability: typeof row.probability === 'number' ? row.probability : null,
      confidence: typeof row.confidence === 'number' ? row.confidence : 0,
      rationale: typeof row.rationale === 'string' ? row.rationale : undefined,
    };
  });
}

function stagger(i: number): CSSProperties {
  return {
    '--i': i,
    animation: 'fadeRise 0.55s var(--ease-luxe) both',
    animationDelay: 'calc(var(--i, 0) * 70ms)',
  } as CSSProperties;
}

export function CalibrationView() {
  const [report, setReport] = useState<ForecastCalibrationReport | null>(null);
  const [bridge, setBridge] = useState<{ online: boolean; degraded: boolean; last_error?: string | null } | null>(
    null,
  );
  const [recent, setRecent] = useState<EdgeAnalysisResult[]>([]);
  const [selectedTicker, setSelectedTicker] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [breakers, setBreakers] = useState<BreakerDecision | null>(null);
  const [lambdaFit, setLambdaFit] = useState<LambdaFit | null | undefined>(undefined);
  const [edgeConfig, setEdgeConfig] = useState<EdgeConfig | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [rep, st, br, ec] = await Promise.all([
        kalshiApi.getForecastCalibrationReport(),
        finceptApi.getBridgeStatus().catch(() => null),
        kalshiApi.evaluateBreakers().catch(() => null),
        kalshiApi.getEdgeConfig().catch(() => null),
      ]);
      setReport(rep);
      if (st) setBridge(st);
      if (br) setBreakers(br);
      if (ec) setEdgeConfig(ec);
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

  const analyzeTop = async (limit: number, deep = false) => {
    setActionBusy(deep ? 'deep' : 'analyze');
    setMessage(null);
    setError(null);
    setSelectedTicker(null);
    try {
      const rows = await kalshiApi.analyzeTopMarketsEdge(limit, deep);
      setRecent(rows);
      setMessage(
        rows.length === 0
          ? 'No markets analyzed (empty tape or all failed). Check Command desk for markets + Fincept bridge.'
          : `Edge Board: ${rows.length} market(s) ranked by |edge_net|${deep ? ' (deep + web)' : ''}. PASS rows included — no fabricated outcomes.`,
      );
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActionBusy(null);
    }
  };

  const selected = useMemo(
    () => recent.find((r) => r.market_ticker === selectedTicker) ?? null,
    [recent, selectedTicker],
  );
  const selectedAgents = useMemo(
    () => (selected ? agentSignalRows(selected.agent_breakdown) : []),
    [selected],
  );

  const runLambdaRefit = async () => {
    setActionBusy('lambda');
    setMessage(null);
    setError(null);
    try {
      const fit = await kalshiApi.refitLambda();
      setLambdaFit(fit);
      setMessage(
        fit
          ? `λ re-fit on n=${fit.n}: λ=${fit.lambda.toFixed(3)} (Brier ${fit.brier_at_fit.toFixed(4)} vs market ${fit.brier_at_market.toFixed(4)}). Saved to edge config.`
          : 'Not enough resolved forecasts with model opinions (need ≥50). Keep resolving markets.',
      );
      if (fit) {
        const ec = await kalshiApi.getEdgeConfig().catch(() => null);
        if (ec) setEdgeConfig(ec);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActionBusy(null);
    }
  };

  const paperSelectedEdge = async (side: 'YES' | 'NO') => {
    if (!selected) return;
    setActionBusy('paper');
    setMessage(null);
    setError(null);
    try {
      const stake = 25;
      const entry =
        side === 'YES' ? selected.p_market : 1 - selected.p_market;
      const feeLine = formatFeePreviewLine(stake, entry);
      const ok = window.confirm(
        `Paper ${side} on ${selected.market_ticker} using agent fair (p_final=${(selected.p_final * 100).toFixed(1)}%)?\nStake ~$${stake}.\n${feeLine}`,
      );
      if (!ok) {
        setActionBusy(null);
        return;
      }
      const res = await kalshiApi.paperFromEdge(selected.market_ticker, side, stake);
      setMessage(
        res.lot_opened
          ? `Edge Board paper lot: ${res.contract_side} ${res.ticker} ~$${res.stake.toFixed(0)}`
          : `Journal only: ${res.final_decision} — ${(res.demotion_notes || []).slice(0, 2).join('; ')}`,
      );
      notifyPaperUpdated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActionBusy(null);
    }
  };

  const exportBrierCsv = () => {
    if (!report) return;
    const rows = [
      ['metric', 'value'],
      ['resolved_count', String(report.resolved_count)],
      ['eligible_count', String(report.eligible_count)],
      ['unresolved_count', String(report.unresolved_count)],
      ['brier_market', String(report.brier_market ?? '')],
      ['brier_final', String(report.brier_final ?? '')],
      ['brier_model', String(report.brier_model ?? '')],
      ['brier_market_on_model_rows', String(report.brier_market_on_model_rows ?? '')],
      ['n_model', String(report.n_model)],
      ['paper_pnl', String(report.paper_pnl ?? '')],
      ['gate_passed', String(report.gate_passed)],
      ...(report.gate_reasons || []).map((r, i) => [`gate_reason_${i + 1}`, r]),
    ];
    const csv = rows.map((r) => r.map((c) => `"${String(c).replace(/"/g, '""')}"`).join(',')).join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `kalshi-brier-export-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    setMessage('Brier gate report exported as CSV.');
  };

  const gateOk = report?.gate_passed === true;
  // Progress tracks the *eligible* sample, not the raw row count: market-only
  // and in-play rows cannot demonstrate model skill, so showing them as
  // progress toward the gate would overstate how close it is.
  const progress =
    report != null ? Math.min(100, (report.eligible_count / 200) * 100) : 0;
  const nModel = report?.n_model ?? 0;
  const lambdaReady = nModel >= 50;
  const lambdaProgress = Math.min(100, (nModel / 50) * 100);
  const modelBeatsMarket =
    report?.brier_model != null &&
    report?.brier_market_on_model_rows != null &&
    report.brier_model <= report.brier_market_on_model_rows;
  const finalBeatsMarket =
    report?.brier_final != null &&
    report?.brier_market != null &&
    report.brier_final <= report.brier_market;

  return (
    <section className="page kalshiPage" aria-label="Calibration surface">
      <header className="kalshiHeader">
        <div>
          <p className="eyebrow">Forecast evidence</p>
          <h2>Calibration</h2>
          <p className="muted">
            Forecast ledger, Brier scores, and the live-trading gate. Evidence only — never synthetic
            outcomes. Live orders stay locked until ≥200 resolved, Brier(p_final) ≤ Brier(p_market), and
            paper P&amp;L &gt; 0. Background auto-grade + paper settle run on the Kalshi poll interval.
          </p>
        </div>
        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          <button type="button" className="ghostBtn" onClick={() => exportBrierCsv()} disabled={!report}>
            Export Brier CSV
          </button>
          <button type="button" className="primaryButton" onClick={() => void refresh()} disabled={loading}>
            {loading ? 'Refreshing…' : 'Refresh report'}
          </button>
        </div>
      </header>

      <div className="insightCard" style={{ marginBottom: '1rem' }} aria-label="Flywheel status">
        <span>Calibration flywheel (Sprint 5)</span>
        <strong className={gateOk ? 'pos' : 'neg'}>
          <LiveDot tone={gateOk ? 'live' : 'idle'} />{' '}
          {gateOk ? 'Gate OPEN — paper/live edge validated path' : 'Gate LOCKED — accumulate resolved rows'}
        </strong>
        <p className="muted">
          Auto-grade poller + paper settle run in the background whenever open lots / pending predictions /
          unresolved forecasts exist. Use Resolve settled forecasts to force a pass. Target ≥200 resolved
          before treating edge as validated.
        </p>
      </div>

      {bridge && (
        <div className={`insightCard ${bridge.online ? 'accent' : ''}`}>
          <span>Fincept agents</span>
          <strong>
            <LiveDot tone={bridge.online ? 'live' : 'idle'} />{' '}
            {bridge.online ? 'Online' : bridge.degraded ? 'Degraded' : 'Offline'}
          </strong>
          <p>
            {bridge.online
              ? 'Agents: technical + contract_tape + news + macro (FRED when keyed). Board scan uses depth=quick.'
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

      {breakers && (
        <div className="insightCard" style={{ marginBottom: '1rem' }} aria-label="Circuit breakers">
          <span>Circuit breakers (§6.4)</span>
          <strong className={breakers.live_orders_allowed ? 'pos' : 'neg'}>
            {breakers.live_orders_allowed ? 'Live orders allowed' : 'Live orders blocked'}
          </strong>
          <p className="muted">
            Stake multiplier {breakers.stake_multiplier.toFixed(2)}
            {breakers.paper_only ? ' · paper-only demotion active' : ''}
          </p>
          {breakers.reasons.length > 0 && (
            <ul className="muted" style={{ margin: '0.5rem 0 0', paddingLeft: '1.25rem' }}>
              {breakers.reasons.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
          )}
          {breakers.state.live_trading_disabled && (
            <button
              type="button"
              className="secondaryButton"
              style={{ marginTop: '0.75rem' }}
              disabled={actionBusy === 'breaker'}
              onClick={() => void (async () => {
                setActionBusy('breaker');
                try {
                  await kalshiApi.manualReenableBreaker();
                  const br = await kalshiApi.evaluateBreakers();
                  setBreakers(br);
                  setMessage('Manual re-enable applied — breakers re-evaluated.');
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                } finally {
                  setActionBusy(null);
                }
              })()}
            >
              {actionBusy === 'breaker' ? 'Working…' : 'Manual re-enable live trading'}
            </button>
          )}
        </div>
      )}

      <div className="mechanicsGrid" style={{ marginBottom: '1rem' }}>
        <div style={stagger(0)}>
          <span>Gate</span>
          <strong className={gateOk ? 'pos' : 'neg'}>{gateOk ? 'OPEN' : 'LOCKED'}</strong>
        </div>
        <div style={stagger(1)}>
          <span title="Model-bearing, pre-event, one row per event — the sample the gate tests">
            Eligible
          </span>
          <strong className="goldText">
            {report?.eligible_count ?? '—'} / 200
          </strong>
        </div>
        <div style={stagger(2)}>
          <span title="Every resolved row, including market-only and in-play rows">
            Resolved (raw)
          </span>
          <strong>{report?.resolved_count ?? '—'}</strong>
        </div>
        <div style={stagger(3)}>
          <span>Unresolved</span>
          <strong>{report?.unresolved_count ?? '—'}</strong>
        </div>
        <div style={stagger(4)}>
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

      <section className="modalSection" aria-label="Gate dashboard">
        <p className="eyebrow" style={{ marginBottom: 4 }}>Scoring</p>
        <h4>Gate dashboard — model vs market</h4>
        <div className="mechanicsGrid">
          <div style={stagger(0)}>
            <span>Brier(p_market)</span>
            <strong className="goldText">{brier(report?.brier_market)}</strong>
          </div>
          <div style={stagger(1)}>
            <span>Brier(p_final)</span>
            <strong className={finalBeatsMarket ? 'pos' : 'goldText'}>
              {brier(report?.brier_final)}
              {finalBeatsMarket ? ' ≤ mkt' : ''}
            </strong>
          </div>
          <div style={stagger(2)}>
            <span>Brier(p_model)</span>
            <strong className={modelBeatsMarket ? 'pos' : 'goldText'}>
              {brier(report?.brier_model)}
              {report && report.n_model > 0 ? ` (n=${report.n_model})` : ''}
              {modelBeatsMarket ? ' ≤ mkt' : ''}
            </strong>
          </div>
          <div style={stagger(3)}>
            <span>Market on model rows</span>
            <strong className="goldText">{brier(report?.brier_market_on_model_rows)}</strong>
          </div>
        </div>
        <p className="muted" style={{ marginTop: '0.5rem' }}>
          Paper equity P&amp;L (realized):{' '}
          <strong className={(report?.paper_pnl ?? 0) > 0 ? 'pos' : 'neg'}>{money(report?.paper_pnl)}</strong>
          {' · '}
          Gate needs paper P&amp;L &gt; 0 after fees.
        </p>
        {report?.gate_reasons && report.gate_reasons.length > 0 && (
          <ul className="muted" style={{ marginTop: '0.75rem', paddingLeft: '1.2rem' }}>
            {report.gate_reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        )}
      </section>

      <section className="modalSection" aria-label="Shrinkage lambda re-fit">
        <p className="eyebrow" style={{ marginBottom: 4 }}>Model tuning</p>
        <h4>Shrinkage λ (§4.1)</h4>
        <p className="muted" style={{ marginBottom: '0.75rem' }}>
          Grid re-fit from resolved forecast rows with model opinions. Successful re-fit persists λ to
          SQLite edge config and applies to analyze / paper edge evaluation.
          {edgeConfig != null ? (
            <>
              {' '}
              Active shrinkage λ: <strong>{edgeConfig.shrinkage_lambda.toFixed(3)}</strong>.
            </>
          ) : null}
        </p>
        <div
          className="insightCard"
          style={{ marginBottom: '0.75rem' }}
          aria-label="Lambda sample progress"
        >
          <span>Model-opinion sample for re-fit</span>
          <strong>
            {nModel} / 50 {lambdaReady ? '· ready' : '· keep resolving'}
          </strong>
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
                width: `${lambdaProgress}%`,
                height: '100%',
                background: lambdaReady ? 'var(--ok, #3d9a5f)' : 'var(--accent, #4a7dff)',
              }}
            />
          </div>
        </div>
        {lambdaFit && (
          <div className="mechanicsGrid" style={{ marginBottom: '0.75rem' }}>
            <div style={stagger(0)}>
              <span>Fitted λ</span>
              <strong>{lambdaFit.lambda.toFixed(3)}</strong>
            </div>
            <div style={stagger(1)}>
              <span>Brier @ fit</span>
              <strong>{brier(lambdaFit.brier_at_fit)}</strong>
            </div>
            <div style={stagger(2)}>
              <span>Brier @ λ=0 (market)</span>
              <strong>{brier(lambdaFit.brier_at_market)}</strong>
            </div>
            <div style={stagger(3)}>
              <span>Rows (n)</span>
              <strong>{lambdaFit.n}</strong>
            </div>
          </div>
        )}
        <button
          type="button"
          className="secondaryButton"
          disabled={actionBusy != null}
          onClick={() => void runLambdaRefit()}
          title={
            lambdaReady
              ? 'Re-fit shrinkage λ from resolved model rows and persist to edge config'
              : `Need ≥50 resolved rows with p_model (have ${nModel})`
          }
        >
          {actionBusy === 'lambda'
            ? 'Fitting…'
            : lambdaReady
              ? 'Re-fit λ from ledger'
              : `Re-fit λ (need ${Math.max(0, 50 - nModel)} more model rows)`}
        </button>
      </section>

      <section className="modalSection" aria-label="Reliability diagram">
        <p className="eyebrow" style={{ marginBottom: 4 }}>Calibration curve</p>
        <h4>Reliability (resolved forecasts)</h4>
        <p className="muted" style={{ marginBottom: '0.75rem' }}>
          Predicted probability vs observed Yes rate per bucket. Points on the diagonal are well calibrated.
        </p>
        <div style={{ display: 'grid', gap: '1rem', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))' }}>
          <ReliabilityDiagram
            title="p_final vs outcomes"
            buckets={report?.reliability_final ?? []}
            compareBuckets={report?.reliability_market}
            compareLabel="p_market"
          />
        </div>
      </section>

      <section className="modalSection" aria-label="Edge Board actions">
        <p className="eyebrow" style={{ marginBottom: 4 }}>Market scanner</p>
        <h4>Edge Board</h4>
        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
          <button
            type="button"
            className="primaryButton"
            disabled={actionBusy != null}
            onClick={() => void analyzeTop(10)}
          >
            {actionBusy === 'analyze' ? 'Analyzing…' : 'Scan top 10 (rank by |edge|)'}
          </button>
          <button
            type="button"
            className="ghostBtn"
            disabled={actionBusy != null}
            onClick={() => void analyzeTop(5)}
          >
            Scan top 5
          </button>
          <button
            type="button"
            className="secondaryButton"
            disabled={actionBusy != null}
            onClick={() => void analyzeTop(3, true)}
          >
            {actionBusy === 'deep' ? 'Deep analyzing…' : 'Deep analyze top 3'}
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
          Board scan uses depth=<code>quick</code> (contract_tape only) and ranks by |edge_net|. Deep top 3
          uses depth=<code>deep</code> (technical + tape + news + web). Single Analyze is{' '}
          <code>standard</code>. Click a row for agent breakdown.
        </p>
      </section>

      {recent.length > 0 && (
        <section className="modalSection" aria-label="Edge Board table">
          <h4>Edge Board (ranked)</h4>
          <div className="tableWrap">
            <table className="dataTable">
              <thead>
                <tr>
                  <th>Ticker</th>
                  <th>p_market</th>
                  <th>p_model</th>
                  <th>p_final</th>
                  <th>|edge|</th>
                  <th>Verdict</th>
                  <th>Agents</th>
                  <th>Conf</th>
                </tr>
              </thead>
              <tbody>
                {recent.map((r) => (
                  <tr
                    key={r.forecast_id}
                    style={{
                      cursor: 'pointer',
                      background:
                        selectedTicker === r.market_ticker
                          ? 'var(--surface-2, rgba(74,125,255,0.12))'
                          : undefined,
                    }}
                    onClick={() =>
                      setSelectedTicker((t) =>
                        t === r.market_ticker ? null : r.market_ticker,
                      )
                    }
                  >
                    <td>
                      <code>{r.market_ticker}</code>
                    </td>
                    <td>{pct(r.p_market)}</td>
                    <td>{r.p_model == null ? '—' : pct(r.p_model)}</td>
                    <td>{pct(r.p_final)}</td>
                    <td>{(absEdge(r) * 100).toFixed(1)}¢</td>
                    <td>{r.verdict}</td>
                    <td>
                      {r.signals_opining}/{r.signals_received}
                    </td>
                    <td>{r.confidence.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {selected && (
            <div
              className="insightCard"
              style={{ marginTop: '0.75rem' }}
              aria-label="Agent breakdown drawer"
            >
              <span>Agent breakdown · {selected.market_ticker}</span>
              <strong>
                {selected.verdict} · conf {selected.confidence.toFixed(2)}
              </strong>
              {selected.verdict_reasons?.length > 0 && (
                <ul className="muted" style={{ margin: '0.5rem 0', paddingLeft: '1.2rem' }}>
                  {selected.verdict_reasons.map((reason) => (
                    <li key={reason}>{reason}</li>
                  ))}
                </ul>
              )}
              {selectedAgents.length === 0 ? (
                <p className="muted">No agent signals on this row (sidecar offline or empty).</p>
              ) : (
                <div className="tableWrap">
                  <table className="dataTable">
                    <thead>
                      <tr>
                        <th>Agent</th>
                        <th>p</th>
                        <th>Conf</th>
                        <th>Rationale</th>
                      </tr>
                    </thead>
                    <tbody>
                      {selectedAgents.map((a) => (
                        <tr key={a.agent}>
                          <td>{a.agent}</td>
                          <td>{a.probability == null ? 'null' : pct(a.probability)}</td>
                          <td>{a.confidence.toFixed(2)}</td>
                          <td className="muted" style={{ maxWidth: 360 }}>
                            {(a.rationale ?? '').slice(0, 180)}
                            {(a.rationale?.length ?? 0) > 180 ? '…' : ''}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              <div style={{ display: 'flex', gap: '0.5rem', marginTop: '0.75rem', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="primaryButton"
                  disabled={actionBusy != null}
                  onClick={() => void paperSelectedEdge('YES')}
                >
                  Paper YES (agent fair)
                </button>
                <button
                  type="button"
                  className="secondaryButton"
                  disabled={actionBusy != null}
                  onClick={() => void paperSelectedEdge('NO')}
                >
                  Paper NO (agent fair)
                </button>
              </div>
              <p className="muted" style={{ marginTop: '0.35rem' }}>
                One-click uses sidecar p_final as fair (LLM not re-queried). Paper cash must cover stake+fee.
              </p>
            </div>
          )}
        </section>
      )}

      {recent.length === 0 && !actionBusy && (
        <section className="modalSection" aria-label="Edge Board empty">
          <p className="muted">
            Edge Board is empty — run a scan when Command desk tape is loaded and (optionally) the Fincept
            bridge is online.
          </p>
        </section>
      )}
    </section>
  );
}
