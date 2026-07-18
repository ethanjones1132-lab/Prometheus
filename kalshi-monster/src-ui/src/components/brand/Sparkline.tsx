import { useId, useMemo } from 'react';

/**
 * Miniature gold sparkline — a smooth SVG polyline with a soft area
 * wash and a glowing terminal dot. Used on market cards and tape rows
 * to give every price a sense of movement.
 */
export function Sparkline({
  points,
  width = 120,
  height = 34,
  tone = 'gold',
}: {
  points: number[];
  width?: number;
  height?: number;
  tone?: 'gold' | 'up' | 'down';
}) {
  const gradientId = useId();
  const geom = useMemo(() => {
    if (points.length < 2) return null;
    const min = Math.min(...points);
    const max = Math.max(...points);
    const span = max - min || 1;
    const pad = 2;
    const stepX = (width - pad * 2) / (points.length - 1);
    const coords = points.map((p, i) => {
      const x = pad + i * stepX;
      const y = pad + (1 - (p - min) / span) * (height - pad * 2 - 4) + 2;
      return [x, y] as const;
    });
    const line = coords.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`).join(' ');
    const area = `${line} L${coords[coords.length - 1][0].toFixed(2)},${height} L${coords[0][0].toFixed(2)},${height} Z`;
    const last = coords[coords.length - 1];
    return { line, area, last };
  }, [points, width, height]);

  if (!geom) return null;

  const stroke = tone === 'up' ? '#86d8a8' : tone === 'down' ? '#e88a80' : '#d4af37';
  const fill = tone === 'up' ? 'rgba(134,216,168,0.14)' : tone === 'down' ? 'rgba(232,138,128,0.12)' : 'rgba(212,175,55,0.13)';

  return (
    <svg
      className="sparkline"
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={fill} />
          <stop offset="100%" stopColor="transparent" />
        </linearGradient>
      </defs>
      <path d={geom.area} fill={`url(#${gradientId})`} stroke="none" />
      <path
        d={geom.line}
        fill="none"
        stroke={stroke}
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ filter: `drop-shadow(0 0 4px ${fill.replace(/0\.1[234]/, '0.5')})` }}
      />
      <circle cx={geom.last[0]} cy={geom.last[1]} r="2.4" fill={stroke} />
    </svg>
  );
}
