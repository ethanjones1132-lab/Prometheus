import { useState, useEffect, useCallback, useRef } from 'react';
import { kalshiApi } from '../services/kalshi';
import type { KalshiCategoryStat, KalshiMarketSummary } from '../types/kalshi';
import { MarketDetailPanel } from './MarketDetailPanel';
import { KalshiPredictionsPanel } from './KalshiPredictionsPanel';

const INITIAL_MARKET_LIMIT = 30;

function formatProb(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '—';
}

function formatSpread(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${(value! * 100).toFixed(1)}¢` : '—';
}

function formatVolume(value: number | undefined | null): string {
  return Number.isFinite(value) ? `$${value!.toLocaleString()}` : '—';
}

export function KalshiView() {
  const [markets, setMarkets] = useState<KalshiMarketSummary[]>([]);
  const [categories, setCategories] = useState<KalshiCategoryStat[]>([]);
  const [selectedCategory, setSelectedCategory] = useState('All');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedMarket, setSelectedMarket] = useState<KalshiMarketSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [marketsReady, setMarketsReady] = useState(false);
  const requestId = useRef(0);

  const loadMarkets = useCallback(async (opts?: { query?: string; category?: string }) => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    const category = opts?.category ?? selectedCategory;
    const query = (opts?.query ?? '').trim();

    try {
      const data = query
        ? await kalshiApi.searchMarkets(query)
        : category === 'All'
          ? await kalshiApi.getTopMarkets(INITIAL_MARKET_LIMIT)
          : await kalshiApi.getMarkets(category);

      if (id !== requestId.current) return;

      setMarkets(data);
      setMarketsReady(true);

      // Categories depend on the Rust-side cache — load after markets, not in parallel
      try {
        const stats = await kalshiApi.getCategoryStats();
        if (id === requestId.current) setCategories(stats);
      } catch {
        // non-fatal
      }
    } catch (e) {
      if (id !== requestId.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, [selectedCategory]);

  // Initial load + category changes only (not every keystroke)
  useEffect(() => {
    void loadMarkets({ category: selectedCategory });
  }, [selectedCategory, loadMarkets]);

  const runSearch = () => {
    void loadMarkets({ query: searchQuery, category: selectedCategory });
  };

  const refreshAll = async () => {
    setRefreshing(true);
    setError(null);
    try {
      await kalshiApi.refresh();
      await loadMarkets({ category: selectedCategory });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="kalshiPage">
      <header className="kalshiHeader">
        <div>
          <h2>Kalshi Markets</h2>
          <p className="muted">Portfolio-aware Kelly · isotonic calibration · price snapshots</p>
        </div>
        <button type="button" className="primaryBtn" onClick={() => void refreshAll()} disabled={refreshing || loading}>
          {refreshing ? 'Refreshing…' : 'Refresh & snapshot'}
        </button>
      </header>

      <div className="kalshiToolbar">
        <input
          className="searchInput"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search ticker or title…"
          onKeyDown={(e) => e.key === 'Enter' && runSearch()}
        />
        <button type="button" className="ghostBtn" onClick={runSearch} disabled={loading}>
          Search
        </button>
      </div>

      <div className="categoryRow">
        <button
          type="button"
          className={`chip ${selectedCategory === 'All' ? 'active' : ''}`}
          onClick={() => setSelectedCategory('All')}
          disabled={loading}
        >
          All
        </button>
        {categories.map((cat) => (
          <button
            key={cat.category}
            type="button"
            className={`chip ${selectedCategory === cat.category ? 'active' : ''}`}
            onClick={() => setSelectedCategory(cat.category)}
            disabled={loading}
          >
            {cat.category} ({cat.count})
          </button>
        ))}
      </div>

      {loading && <p className="muted pad">Loading markets…</p>}
      {error && <p className="error pad">{error}</p>}

      {!loading && (
        <div className="marketGrid">
          {markets.map((market) => (
            <button
              key={market.ticker}
              type="button"
              className="marketCard"
              onClick={() => setSelectedMarket(market)}
            >
              <div className="marketCardTop">
                <code>{market.ticker}</code>
                <span className="chip small">{market.category}</span>
              </div>
              <h3>{market.title}</h3>
              <div className="marketStats">
                <span>YES {formatProb(market.yes_prob_pct)}</span>
                <span>Spread {formatSpread(market.spread)}</span>
                <span>Vol {formatVolume(market.volume_24h)}</span>
              </div>
            </button>
          ))}
        </div>
      )}

      {!loading && markets.length === 0 && !error && (
        <p className="muted pad">No markets found.</p>
      )}

      {marketsReady && <KalshiPredictionsPanel />}

      {selectedMarket && (
        <MarketDetailPanel market={selectedMarket} onClose={() => setSelectedMarket(null)} />
      )}
    </div>
  );
}