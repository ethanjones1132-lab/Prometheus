import { useId } from 'react';
import type { KalshiPriceHistory } from '../types/kalshi';

interface Props {
  history: KalshiPriceHistory | null;
  loading?: boolean;
}

const GOLD = '#d4af37';
const GOLD_BRIGHT = '#ecd27a';
const GRID_LINE = 'rgba(212, 175, 55, 0.08)';
const AXIS_TEXT = '#716a58';

export function PriceHistoryChart({ history, loading }: Props) {
  const gradientId = useId();

  if (loading) {
    return <p className="muted small">Loading price history…</p>;
  }
  if (!history || history.snapshots.length < 2) {
    return <p className="muted small">No snapshot history yet — refresh markets to start tracking.</p>;
  }

  const points = history.snapshots;
  const w = 320;
  const h = 96;
  const padX = 8;
  const padTop = 12;
  const padBottom = 10;
  const probs = points.map((p) => p.yes_prob_pct);
  const dataMin = Math.min(...probs);
  const dataMax = Math.max(...probs);
  const min = dataMin - 1;
  const max = dataMax + 1;
  const range = Math.max(max - min, 1);

  const toX = (i: number) => padX + (i / (points.length - 1)) * (w - padX * 2);
  const toY = (v: number) => h - padBottom - ((v - min) / range) * (h - padTop - padBottom);

  const line = points.map((p, i) => `${toX(i)},${toY(p.yes_prob_pct)}`).join(' ');
  const area = `${padX},${h - padBottom} ${line} ${w - padX},${h - padBottom}`;
  const lastX = toX(points.length - 1);
  const lastY = toY(probs[probs.length - 1]);
  const gridLevels = [dataMax, (dataMax + dataMin) / 2, dataMin];

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
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="rgba(212, 175, 55, 0.28)" />
            <stop offset="100%" stopColor="rgba(212, 175, 55, 0)" />
          </linearGradient>
        </defs>
        {gridLevels.map((level) => (
          <line
            key={level}
            x1={padX}
            x2={w - padX}
            y1={toY(level)}
            y2={toY(level)}
            stroke={GRID_LINE}
            strokeWidth="1"
          />
        ))}
        <text x={padX + 2} y={toY(dataMax) - 3} fill={AXIS_TEXT} fontSize="8">
          {dataMax.toFixed(1)}%
        </text>
        <text x={padX + 2} y={toY(dataMin) + 9} fill={AXIS_TEXT} fontSize="8">
          {dataMin.toFixed(1)}%
        </text>
        <polygon points={area} fill={`url(#${gradientId})`} />
        <polyline
          fill="none"
          stroke={GOLD}
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          points={line}
        />
        <circle cx={lastX} cy={lastY} r="5" fill="rgba(212, 175, 55, 0.18)" />
        <circle
          cx={lastX}
          cy={lastY}
          r="2.6"
          fill={GOLD_BRIGHT}
          style={{ filter: 'drop-shadow(0 0 5px rgba(212, 175, 55, 0.9))' }}
        />
      </svg>
    </div>
  );
}
