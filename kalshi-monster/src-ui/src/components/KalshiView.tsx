import { useState, useEffect, useCallback, useRef } from 'react';
import { kalshiApi } from '../services/kalshi';
import type { KalshiCategoryStat, KalshiMarketSummary } from '../types/kalshi';
import { MarketDetailPanel } from './MarketDetailPanel';
import { KalshiPredictionsPanel } from './KalshiPredictionsPanel';

const INITIAL_MARKET_LIMIT = 30;

function formatProb(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${value!.toFixed(1)}%` : '-';
}

function formatSpread(value: number | undefined | null): string {
  return Number.isFinite(value) ? `${(value! * 100).toFixed(1)}c` : '-';
}

function formatVolume(value: number | undefined | null): string {
  return Number.isFinite(value) ? `$${value!.toLocaleString()}` : '-';
}

function formatDateLabel(value?: string | null): string {
  if (!value) return 'No close listed';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Invalid close';
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    timeZone: 'UTC',
  }).format(date);
}

function cacheLabel(status: string, partial: boolean): string {
  if (partial || status === 'partial') return 'Partial catalog';
  if (status === 'full') return 'Full catalog';
  return 'Cold cache';
}

interface KalshiViewProps {
  onAnalyzeMarket?: (prompt: string) => void;
}

export function KalshiView({ onAnalyzeMarket }: KalshiViewProps = {}) {
  const [markets, setMarkets] = useState<KalshiMarketSummary[]>([]);
  const [categories, setCategories] = useState<KalshiCategoryStat[]>([]);
  const [cacheStatus, setCacheStatus] = useState('cold');
  const [cacheAgeSecs, setCacheAgeSecs] = useState<number | null>(null);
  const [partialCatalog, setPartialCatalog] = useState(true);
  const [lastRefreshAt, setLastRefreshAt] = useState<string | null>(null);
  const [marketCount, setMarketCount] = useState(0);
  const [categoryCount, setCategoryCount] = useState(0);
  const [dataQualityNotes, setDataQualityNotes] = useState<string[]>([]);
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
      if (!query && category === 'All') {
        const bootstrap = await kalshiApi.getDashboardBootstrap(INITIAL_MARKET_LIMIT);
        if (id !== requestId.current) return;
        setMarkets(bootstrap.markets);
        setCategories(bootstrap.categories);
        setCacheStatus(bootstrap.cache_status);
        setCacheAgeSecs(bootstrap.cache_age_secs ?? null);
        setPartialCatalog(bootstrap.partial_catalog);
        setLastRefreshAt(bootstrap.last_refresh_at ?? null);
        setMarketCount(bootstrap.market_count);
        setCategoryCount(bootstrap.category_count);
        setDataQualityNotes(bootstrap.data_quality_notes);
        setMarketsReady(true);
        return;
      }

      const data = query
        ? await kalshiApi.searchMarkets(query)
        : await kalshiApi.getMarkets(category);

      if (id !== requestId.current) return;

      setMarkets(data);
      setMarketsReady(true);

      try {
        const stats = await kalshiApi.getCategoryStats();
        if (id === requestId.current) setCategories(stats);
      } catch {
        // Category stats are helpful but not required for search results.
      }
    } catch (e) {
      if (id !== requestId.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, [selectedCategory]);

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
          <p className="muted">Portfolio-aware Kelly, calibration, price snapshots, and paper decisions.</p>
        </div>
        <button type="button" className="primaryBtn" onClick={() => void refreshAll()} disabled={refreshing || loading}>
          {refreshing ? 'Refreshing...' : 'Refresh and snapshot'}
        </button>
      </header>

      <div className="dashboardStatus" aria-label="Market data status">
        <span>{cacheLabel(cacheStatus, partialCatalog)}</span>
        {cacheAgeSecs != null && <span>Cache age {cacheAgeSecs}s</span>}
        {lastRefreshAt && <span>Last refresh {formatDateLabel(lastRefreshAt)}</span>}
        <span>Markets {marketCount}</span>
        <span>Categories {categoryCount}</span>
        {dataQualityNotes.map((note) => (
          <span key={note} className="diagnosticNote">{note}</span>
        ))}
      </div>

      <div className="kalshiToolbar">
        <input
          className="searchInput"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search ticker or market title..."
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

      {loading && <p className="muted pad">Loading markets...</p>}
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
                <span>Liq {formatVolume(market.liquidity)}</span>
                <span>Status {market.status}</span>
                <span>Close {formatDateLabel(market.close_time)}</span>
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
        <MarketDetailPanel
          market={selectedMarket}
          onClose={() => setSelectedMarket(null)}
          onAnalyzeMarket={onAnalyzeMarket}
        />
      )}
    </div>
  );
}
