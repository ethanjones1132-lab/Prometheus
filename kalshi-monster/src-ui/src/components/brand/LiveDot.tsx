/**
 * Pulsing status ember — green for live connections, gold for
 * working/streaming, dim when idle.
 */
export function LiveDot({ tone = 'live' }: { tone?: 'live' | 'gold' | 'idle' }) {
  const cls = tone === 'gold' ? 'liveDot gold' : tone === 'idle' ? 'liveDot idle' : 'liveDot';
  return <span className={cls} aria-hidden="true" />;
}
