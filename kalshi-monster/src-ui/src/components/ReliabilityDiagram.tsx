import type { ReliabilityBucket } from '../types/kalshi';

const W = 320;
const H = 200;
const PAD = 28;

type Props = {
  title: string;
  buckets: ReliabilityBucket[];
  /** Optional second series (e.g. market vs final). */
  compareBuckets?: ReliabilityBucket[];
  compareLabel?: string;
};

function bucketPoints(buckets: ReliabilityBucket[]): { x: number; y: number; n: number }[] {
  return buckets
    .filter((b) => b.count > 0)
    .map((b) => ({
      x: b.predicted_mean,
      y: b.observed_freq,
      n: b.count,
    }));
}

export function ReliabilityDiagram({ title, buckets, compareBuckets, compareLabel }: Props) {
  const pts = bucketPoints(buckets);
  const cmp = compareBuckets ? bucketPoints(compareBuckets) : [];
  const hasData = pts.length > 0 || cmp.length > 0;

  const toX = (p: number) => PAD + p * (W - PAD * 2);
  const toY = (p: number) => H - PAD - p * (H - PAD * 2);

  const perfect = `M ${toX(0)} ${toY(0)} L ${toX(1)} ${toY(1)}`;

  const poly = (points: { x: number; y: number }[]) =>
    points.length === 0
      ? ''
      : points
          .map((p, i) => `${i === 0 ? 'M' : 'L'} ${toX(p.x)} ${toY(p.y)}`)
          .join(' ');

  return (
    <div className="insightCard" aria-label={title}>
      <span>{title}</span>
      {!hasData ? (
        <p className="muted" style={{ marginTop: '0.5rem' }}>
          No resolved forecasts yet — diagram fills as outcomes land in the ledger.
        </p>
      ) : (
        <svg
          viewBox={`0 0 ${W} ${H}`}
          width="100%"
          style={{ maxWidth: 360, marginTop: '0.5rem' }}
          role="img"
          aria-label={`${title} reliability chart`}
        >
          {[0.25, 0.5, 0.75].map((g) => (
            <g key={g}>
              <line x1={toX(g)} y1={PAD} x2={toX(g)} y2={H - PAD} stroke="rgba(212,175,55,0.08)" />
              <line x1={PAD} y1={toY(g)} x2={W - PAD} y2={toY(g)} stroke="rgba(212,175,55,0.08)" />
            </g>
          ))}
          <line x1={PAD} y1={H - PAD} x2={W - PAD} y2={H - PAD} stroke="rgba(212,175,55,0.22)" />
          <line x1={PAD} y1={PAD} x2={PAD} y2={H - PAD} stroke="rgba(212,175,55,0.22)" />
          <path d={perfect} fill="none" stroke="rgba(240,234,219,0.2)" strokeDasharray="4 4" />
          {cmp.length > 0 && (
            <path
              d={poly(cmp)}
              fill="none"
              stroke="#a89e86"
              strokeWidth={1.75}
              opacity={0.75}
            />
          )}
          {pts.length > 0 && (
            <path
              d={poly(pts)}
              fill="none"
              stroke="#d4af37"
              strokeWidth={2.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              style={{ filter: 'drop-shadow(0 0 5px rgba(212,175,55,0.45))' }}
            />
          )}
          {pts.map((p) => (
            <circle
              key={`${p.x}-${p.y}`}
              cx={toX(p.x)}
              cy={toY(p.y)}
              r={3 + Math.min(6, Math.sqrt(p.n))}
              fill="#ecd27a"
              stroke="#d4af37"
              strokeWidth={1}
              opacity={0.95}
            />
          ))}
          <text x={PAD} y={H - 6} fontSize={10} fill="#716a58">
            0
          </text>
          <text x={W - PAD - 8} y={H - 6} fontSize={10} fill="#716a58">
            1
          </text>
          <text x={W / 2} y={H - 6} fontSize={10} fill="#716a58" textAnchor="middle">
            pred
          </text>
          <text x={4} y={PAD + 4} fontSize={10} fill="#716a58">
            freq
          </text>
        </svg>
      )}
      {hasData && compareBuckets && compareLabel && (
        <p className="muted" style={{ fontSize: '0.85rem', marginTop: '0.35rem' }}>
          <span style={{ color: '#d4af37' }}>●</span> p_final &nbsp;
          <span style={{ color: '#a89e86' }}>—</span> {compareLabel}
        </p>
      )}
    </div>
  );
}