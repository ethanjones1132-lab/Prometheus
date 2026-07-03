import { useCallback, useEffect, useState } from 'react';
import { kalshiApi } from '../services/kalshi';
import type { KalshiPrediction, PaperAnalytics, PaperPosition } from '../types/kalshi';
import { kalshiBetWon } from '../types/kalshi';

function formatDollars(value?: number | null): string {
  if (value == null || !Number.isFinite(value)) return '-';
  return `$${value.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

function formatCents(value?: number | null): string {
  if (value == null || !Number.isFinite(value)) return 'No mark';
  return `${value.toFixed(1)}c`;
}

export function KalshiPredictionsPanel() {
  const [predictions, setPredictions] = useState<KalshiPrediction[]>([]);
  const [analytics, setAnalytics] = useState<PaperAnalytics | null>(null);
  const [positions, setPositions] = useState<PaperPosition[]>([]);
  const [loading, setLoading] = useState(true);
  const [grading, setGrading] = useState(false);
  const [settling, setSettling] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [data, paper, openPositions] = await Promise.all([
        kalshiApi.getPredictions(),
        kalshiApi.getPaperAnalytics().catch(() => null),
        kalshiApi.getPaperPositions().catch(() => []),
      ]);
      setPredictions(data);
      setAnalytics(paper);
      setPositions(openPositions);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const gradePending = async () => {
    setGrading(true);
    setMessage(null);
    try {
      const summary = await kalshiApi.gradePending();
      setMessage(`Graded ${summary.graded} (${summary.wins}W/${summary.losses}L, $${summary.total_pnl.toFixed(2)})`);
      await load();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setGrading(false);
    }
  };

  const settlePaper = async () => {
    setSettling(true);
    setMessage(null);
    try {
      const summary = await kalshiApi.settlePaperPositions();
      setMessage(`Settled ${summary.settled} (${summary.wins}W/${summary.losses}L, $${summary.total_pnl.toFixed(2)})`);
      await load();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setSettling(false);
    }
  };

  const resetPaper = async () => {
    const confirmed = window.confirm('Reset paper account to $10,000 and clear paper trade history?');
    if (!confirmed) return;

    setResetting(true);
    setMessage(null);
    try {
      const account = await kalshiApi.resetPaperAccount(10000);
      setMessage(`Paper account reset to ${formatDollars(account.balance_dollars)}`);
      await load();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setResetting(false);
    }
  };

  return (
    <section className="predictionsPanel">
      <div className="panelToolbar">
        <h4>Kalshi paper trades</h4>
        <button type="button" className="ghostBtn" onClick={() => void load()} disabled={loading}>
          Refresh
        </button>
        <button type="button" className="primaryBtn" onClick={() => void gradePending()} disabled={grading}>
          {grading ? 'Grading...' : 'Grade pending'}
        </button>
        <button type="button" className="ghostBtn" onClick={() => void settlePaper()} disabled={settling || loading}>
          {settling ? 'Settling...' : 'Settle paper'}
        </button>
        <button type="button" className="ghostBtn danger" onClick={() => void resetPaper()} disabled={resetting || loading}>
          {resetting ? 'Resetting...' : 'Reset paper'}
        </button>
      </div>

      {analytics && (
        <div className="paperSummary">
          <div>
            <span className="muted">Paper equity</span>
            <strong>${analytics.equity.toFixed(2)}</strong>
          </div>
          <div>
            <span className="muted">Cash</span>
            <strong>${analytics.cash_balance.toFixed(2)}</strong>
          </div>
          <div>
            <span className="muted">Open</span>
            <strong>{analytics.open_positions}</strong>
          </div>
          <div>
            <span className="muted">Return</span>
            <strong>{analytics.total_return_pct.toFixed(1)}%</strong>
          </div>
          <div>
            <span className="muted">Win rate</span>
            <strong>{analytics.win_rate.toFixed(0)}%</strong>
          </div>
          <div>
            <span className="muted">Unrealized</span>
            <strong className={analytics.unrealized_pnl >= 0 ? 'pos' : 'neg'}>{formatDollars(analytics.unrealized_pnl)}</strong>
          </div>
        </div>
      )}

      <section className="paperPortfolio" aria-label="Paper portfolio">
        <div className="paperPortfolioHeader">
          <h5>Paper portfolio</h5>
          <span className="muted small">{positions.length} open position{positions.length === 1 ? '' : 's'}</span>
        </div>
        {positions.length > 0 ? (
          <div className="positionsTable">
            <div className="positionsTableHeader">
              <span>Market</span>
              <span>Side</span>
              <span>Entry</span>
              <span>Mark</span>
              <span>Value</span>
              <span>PnL</span>
            </div>
            {positions.map((position) => (
              <div key={`${position.ticker}-${position.side}`} className="positionRow">
                <div>
                  <code>{position.ticker}</code>
                  <span className="muted small">{position.title}</span>
                </div>
                <span>{position.side} x{position.total_qty.toLocaleString()}</span>
                <span>Entry {formatCents(position.avg_entry_price_cents)}</span>
                <span>Mark {formatCents(position.mark_price_cents)}</span>
                <span>Value {formatDollars(position.market_value_dollars)}</span>
                <span className={(position.unrealized_pnl_dollars ?? 0) >= 0 ? 'pos' : 'neg'}>
                  PnL {formatDollars(position.unrealized_pnl_dollars)}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted small">No open paper positions.</p>
        )}
      </section>

      {message && <p className="muted small">{message}</p>}
      {loading && <p className="muted">Loading predictions...</p>}
      <div className="predList">
        {predictions.map((pred) => {
          const won = kalshiBetWon(pred);
          const pending = pred.actual_outcome == null;
          return (
            <article
              key={pred.id}
              className={`predCard ${pending ? 'pending' : won ? 'win' : 'loss'}`}
            >
              <header>
                <code>{pred.ticker}</code>
                <span>{pred.contract_side ?? pred.pick_type ?? '-'}</span>
              </header>
              <p>{pred.title}</p>
              <div className="predMeta">
                <span>Fair {pred.predicted_probability.toFixed(1)}%</span>
                <span>Stake ${pred.stake_amount.toFixed(2)}</span>
                {pred.market_price_at_entry != null && (
                  <span>Entry {pred.market_price_at_entry.toFixed(1)}%</span>
                )}
                {pred.pnl != null && <span>PnL ${pred.pnl.toFixed(2)}</span>}
              </div>
              {!pending && (
                <strong className={won ? 'pos' : 'neg'}>{won ? 'Win' : 'Loss'}</strong>
              )}
            </article>
          );
        })}
        {!loading && predictions.length === 0 && (
          <p className="muted">No Kalshi predictions yet - record a paper trade from a market detail panel.</p>
        )}
      </div>
    </section>
  );
}
