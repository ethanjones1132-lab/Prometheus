import { useCallback, useEffect, useState } from 'react';
import { finceptApi } from '../services/tauri';

type TrackerRow = {
  ticker?: string;
  name?: string;
  category?: string;
  last_price?: number | null;
  change_pct?: number | null;
  error?: string;
};

type TrackerPayload = {
  category?: string;
  categories?: string[];
  instruments?: TrackerRow[];
  fetched_at?: number;
};

const CATEGORY_LABELS: Record<string, string> = {
  stocks: 'Stocks',
  etfs: 'ETFs',
  crypto: 'Crypto',
  commodities: 'Commodities',
  forex: 'FX',
  bonds: 'Rates / bonds',
};

function formatPrice(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  if (Math.abs(value) >= 100) return value.toFixed(2);
  if (Math.abs(value) >= 1) return value.toFixed(4);
  return value.toFixed(6);
}

function formatPct(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(2)}%`;
}

export function WorldMarketsView() {
  const [status, setStatus] = useState<{ online: boolean; degraded: boolean; last_error?: string | null } | null>(null);
  const [data, setData] = useState<TrackerPayload | null>(null);
  const [category, setCategory] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const st = await finceptApi.getBridgeStatus();
      setStatus(st);
      if (!st.online) {
        setData(null);
        return;
      }
      const payload = await finceptApi.getMarketTracker(category || null);
      setData(payload as TrackerPayload);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [category]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const categories =
    data?.categories ??
    (data?.category ? [data.category] : Object.keys(CATEGORY_LABELS));

  const rows = data?.instruments ?? [];

  return (
    <section className="page kalshiPage">
      <header className="kalshiHeader">
        <div>
          <h2>World markets</h2>
          <p className="muted">
            Cross-asset spot context from the Fincept sidecar (yfinance). Use for macro-linked Kalshi contracts — not trade signals.
          </p>
        </div>
        <button type="button" className="primaryButton" onClick={() => void refresh()} disabled={loading}>
          {loading ? 'Refreshing…' : 'Refresh'}
        </button>
      </header>

      {status && (
        <div className={`insightCard ${status.online ? 'accent' : ''}`}>
          <span>Fincept bridge</span>
          <strong>{status.online ? 'Online' : status.degraded ? 'Degraded' : 'Offline'}</strong>
          <p>
            {status.online
              ? 'Analyst chat will append live snapshot context on each message.'
              : 'Start the sidecar from Settings (dev) or restart the app. Analyst stays Kalshi-only until online.'}
            {status.last_error ? ` — ${status.last_error}` : ''}
          </p>
        </div>
      )}

      {error && <p className="errorText">{error}</p>}

      {status?.online && (
        <>
          <div className="filterRow" style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap', marginBottom: '1rem' }}>
            <button
              type="button"
              className={category === '' ? 'navButton active' : 'navButton'}
              onClick={() => setCategory('')}
            >
              All categories
            </button>
            {categories.map((cat) => (
              <button
                key={cat}
                type="button"
                className={category === cat ? 'navButton active' : 'navButton'}
                onClick={() => setCategory(cat)}
              >
                {CATEGORY_LABELS[cat] ?? cat}
              </button>
            ))}
          </div>

          <div className="marketGrid" style={{ display: 'grid', gap: '0.75rem' }}>
            {rows.length === 0 && !loading && <p className="muted">No instruments in this view.</p>}
            {rows.map((row) => (
              <article key={`${row.category}-${row.ticker}`} className="insightCard">
                <span>{row.category ? CATEGORY_LABELS[row.category] ?? row.category : 'Instrument'}</span>
                <strong>
                  {row.ticker}
                  {row.name ? ` — ${row.name}` : ''}
                </strong>
                <p>
                  Last: {formatPrice(row.last_price ?? null)}
                  {row.change_pct != null && Number.isFinite(row.change_pct) && (
                    <> · 1d {formatPct(row.change_pct)}</>
                  )}
                  {row.error && <> · {row.error}</>}
                </p>
              </article>
            ))}
          </div>
        </>
      )}
    </section>
  );
}