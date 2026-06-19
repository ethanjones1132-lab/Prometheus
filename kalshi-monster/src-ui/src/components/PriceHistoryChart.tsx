import type { KalshiPriceHistory } from '../types/kalshi';

interface Props {
  history: KalshiPriceHistory | null;
  loading?: boolean;
}

export function PriceHistoryChart({ history, loading }: Props) {
  if (loading) {
    return <p className="muted small">Loading price history…</p>;
  }
  if (!history || history.snapshots.length < 2) {
    return <p className="muted small">No snapshot history yet — refresh markets to start tracking.</p>;
  }

  const points = history.snapshots;
  const w = 320;
  const h = 80;
  const pad = 8;
  const probs = points.map((p) => p.yes_prob_pct);
  const min = Math.min(...probs) - 1;
  const max = Math.max(...probs) + 1;
  const range = Math.max(max - min, 1);

  const coords = points.map((p, i) => {
    const x = pad + (i / (points.length - 1)) * (w - pad * 2);
    const y = h - pad - ((p.yes_prob_pct - min) / range) * (h - pad * 2);
    return `${x},${y}`;
  });

  return (
    <div className="priceChart">
      <div className="priceChartMeta">
        <span>Open {history.opening_yes_prob?.toFixed(1) ?? '—'}%</span>
        <span>Now {history.current_yes_prob?.toFixed(1) ?? '—'}%</span>
        <span className={history.prob_change && history.prob_change >= 0 ? 'pos' : 'neg'}>
          Δ {(history.prob_change ?? 0) >= 0 ? '+' : ''}
          {(history.prob_change ?? 0).toFixed(1)} pts
        </span>
      </div>
      <svg viewBox={`0 0 ${w} ${h}`} className="priceChartSvg" role="img" aria-label="YES probability history">
        <polyline fill="none" stroke="#58a6ff" strokeWidth="2" points={coords.join(' ')} />
      </svg>
    </div>
  );
}