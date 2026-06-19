import { useCallback, useEffect, useState } from 'react';
import { kalshiApi } from '../services/kalshi';
import type { KalshiPrediction } from '../types/kalshi';
import { kalshiBetWon } from '../types/kalshi';

export function KalshiPredictionsPanel() {
  const [predictions, setPredictions] = useState<KalshiPrediction[]>([]);
  const [loading, setLoading] = useState(true);
  const [grading, setGrading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await kalshiApi.getPredictions();
      setPredictions(data);
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

  return (
    <section className="predictionsPanel">
      <div className="panelToolbar">
        <h4>Kalshi paper trades</h4>
        <button type="button" className="ghostBtn" onClick={() => void load()} disabled={loading}>
          Refresh
        </button>
        <button type="button" className="primaryBtn" onClick={() => void gradePending()} disabled={grading}>
          {grading ? 'Grading…' : 'Grade pending'}
        </button>
      </div>
      {message && <p className="muted small">{message}</p>}
      {loading && <p className="muted">Loading predictions…</p>}
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
                <span>{pred.contract_side ?? pred.pick_type ?? '—'}</span>
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
          <p className="muted">No Kalshi predictions yet — record a paper trade from a market detail panel.</p>
        )}
      </div>
    </section>
  );
}