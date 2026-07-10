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
          <line x1={PAD} y1={H - PAD} x2={W - PAD} y2={H - PAD} stroke="var(--border,#444)" />
          <line x1={PAD} y1={PAD} x2={PAD} y2={H - PAD} stroke="var(--border,#444)" />
          <path d={perfect} fill="none" stroke="var(--muted,#888)" strokeDasharray="4 3" />
          {cmp.length > 0 && (
            <path
              d={poly(cmp)}
              fill="none"
              stroke="var(--muted,#888)"
              strokeWidth={2}
              opacity={0.85}
            />
          )}
          {pts.length > 0 && (
            <path
              d={poly(pts)}
              fill="none"
              stroke="var(--accent,#4a7dff)"
              strokeWidth={2.5}
            />
          )}
          {pts.map((p) => (
            <circle
              key={`${p.x}-${p.y}`}
              cx={toX(p.x)}
              cy={toY(p.y)}
              r={3 + Math.min(6, Math.sqrt(p.n))}
              fill="var(--accent,#4a7dff)"
              opacity={0.9}
            />
          ))}
          <text x={PAD} y={H - 6} fontSize={10} fill="var(--muted,#888)">
            0
          </text>
          <text x={W - PAD - 8} y={H - 6} fontSize={10} fill="var(--muted,#888)">
            1
          </text>
          <text x={4} y={PAD + 4} fontSize={10} fill="var(--muted,#888)">
            freq
          </text>
        </svg>
      )}
      {hasData && compareBuckets && compareLabel && (
        <p className="muted" style={{ fontSize: '0.85rem', marginTop: '0.35rem' }}>
          <span style={{ color: 'var(--accent,#4a7dff)' }}>●</span> p_final &nbsp;
          <span style={{ color: 'var(--muted,#888)' }}>—</span> {compareLabel}
        </p>
      )}
    </div>
  );
}