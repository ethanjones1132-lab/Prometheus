import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { kalshiApi } from '../services/kalshi';
import { mlApi } from '../services/tauri';
import type { KalshiCategoryStat, KalshiMarketSummary, MLPhase3DashboardSummary } from '../types/kalshi';
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

function formatCompactMoney(value: number | undefined | null): string {
  if (!Number.isFinite(value)) return '-';
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(value!);
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

function mlPhase3DashboardLabel(summary: MLPhase3DashboardSummary): string {
  const base = `ML Phase 3: ${summary.trainable_non_sports_categories}/${summary.non_sports_sidecar_target} sidecar categories`;
  const journal = ` · ${summary.kalshi_resolved_predictions} resolved Kalshi paper rows`;
  const pending =
    summary.kalshi_pending_predictions > 0
      ? ` · ${summary.kalshi_pending_predictions} pending grades`
      : '';
  let retrain = '';
  if (summary.auto_retrain_eligible) {
    retrain = ' · auto-retrain on grade active';
  } else if ((summary.resolved_until_auto_retrain ?? 0) > 0) {
    retrain = ` · auto-retrain in ${summary.resolved_until_auto_retrain} more resolved rows`;
  }
  if (summary.phase_3_data_metric_ready) {
    return `${base} ready${journal}${pending}${retrain}${mlArtifactsLabel(summary)}`;
  }
  if (
    summary.next_sidecar_category != null &&
    summary.next_sidecar_samples_needed != null &&
    summary.next_sidecar_samples_needed > 0
  ) {
    return `${base}${journal}${pending}${retrain} · next: ${summary.next_sidecar_category} (+${summary.next_sidecar_samples_needed} graded)${mlArtifactsLabel(summary)}`;
  }
  return `${base}${journal}${pending}${retrain}${mlArtifactsLabel(summary)}`;
}

function mlArtifactsLabel(summary: MLPhase3DashboardSummary): string {
  const unified = summary.unified_model_on_disk ? 'unified on disk' : 'no unified model';
  const sidecars = summary.active_sidecar_count ?? 0;
  let cv = '';
  if (summary.unified_model_on_disk && summary.unified_cv_accuracy_mean != null) {
    const pct = (summary.unified_cv_accuracy_mean * 100).toFixed(1);
    if (summary.unified_cv_accuracy_std != null) {
      const stdPct = (summary.unified_cv_accuracy_std * 100).toFixed(1);
      cv = ` · unified CV ${pct}% ±${stdPct}%`;
    } else {
      cv = ` · unified CV ${pct}%`;
    }
  }
  return ` · ML artifacts: ${unified}, ${sidecars} sidecar${sidecars === 1 ? '' : 's'}${cv}`;
}

function opportunityScore(market: KalshiMarketSummary): number {
  const liquidityScore = Math.min(market.liquidity / 1000, 75);
  const volumeScore = Math.min(market.volume_24h / 5000, 45);
  const spreadPenalty = Math.max(market.spread * 100, 0) * 5;
  const actionablePriceBonus = market.yes_prob_pct > 8 && market.yes_prob_pct < 92 ? 12 : 0;
  return liquidityScore + volumeScore + actionablePriceBonus - spreadPenalty;
}

function marketBias(market: KalshiMarketSummary): string {
  if (market.yes_prob_pct >= 65) return 'Consensus YES';
  if (market.yes_prob_pct <= 35) return 'Consensus NO';
  return 'Live disagreement';
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
  const [mlPhase3, setMlPhase3] = useState<MLPhase3DashboardSummary | null>(null);
  const [gradingPending, setGradingPending] = useState(false);
  const [gradeFlash, setGradeFlash] = useState<string | null>(null);
  const [mlTraining, setMlTraining] = useState(false);
  const [mlTrainFlash, setMlTrainFlash] = useState<string | null>(null);
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
        setMlPhase3(bootstrap.ml_phase3 ?? null);
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

  const gradePendingKalshi = async () => {
    setGradingPending(true);
    setGradeFlash(null);
    try {
      const summary = await kalshiApi.gradePending();
      setGradeFlash(
        `Graded ${summary.graded} (${summary.wins}W/${summary.losses}L, $${summary.total_pnl.toFixed(2)})`,
      );
      await loadMarkets({ category: selectedCategory });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setGradingPending(false);
    }
  };

  const trainMlFromDashboard = async () => {
    setMlTraining(true);
    setMlTrainFlash(null);
    try {
      const result = await mlApi.trainModel();
      if (result.status === 'trained') {
        const acc =
          result.cv_accuracy_mean != null
            ? ` — CV ${(result.cv_accuracy_mean * 100).toFixed(1)}%`
            : '';
        setMlTrainFlash(`ML trained (${result.samples ?? 0} samples${acc})`);
      } else {
        setMlTrainFlash(result.message ?? 'ML training finished');
      }
      await loadMarkets({ category: selectedCategory });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setMlTraining(false);
    }
  };

  const visibleLiquidity = useMemo(
    () => markets.reduce((sum, market) => sum + (Number.isFinite(market.liquidity) ? market.liquidity : 0), 0),
    [markets],
  );

  const visibleVolume = useMemo(
    () => markets.reduce((sum, market) => sum + (Number.isFinite(market.volume_24h) ? market.volume_24h : 0), 0),
    [markets],
  );

  const averageSpread = useMemo(() => {
    if (markets.length === 0) return null;
    return markets.reduce((sum, market) => sum + market.spread, 0) / markets.length;
  }, [markets]);

  const topCategories = useMemo(
    () =>
      [...categories]
        .sort((a, b) => (b.volume_24h || 0) - (a.volume_24h || 0) || b.count - a.count)
        .slice(0, 4),
    [categories],
  );

  const highValueMarkets = useMemo(
    () =>
      [...markets]
        .filter((market) => market.status.toLowerCase() === 'open')
        .sort((a, b) => opportunityScore(b) - opportunityScore(a))
        .slice(0, 4),
    [markets],
  );

  const dashboardTips = useMemo(() => {
    const tips = [
      'Start with tight spreads and visible liquidity before arguing with the market price.',
      'Use the analyst on one market at a time: thesis, breakpoints, side, then stake.',
      'Record paper decisions before the close so the portfolio view can grade the process.',
    ];

    if (partialCatalog) {
      tips.unshift('Refresh the catalog before sizing. Partial mode is fast, but it is not the final board.');
    }

    if (averageSpread != null && averageSpread > 0.08) {
      tips.unshift('Spreads are wide on the visible board. Demand a larger edge or watch for a better entry.');
    }

    if (mlPhase3 != null && mlPhase3.kalshi_pending_predictions > 0) {
      tips.unshift(
        `You have ${mlPhase3.kalshi_pending_predictions} pending Kalshi grades — use Grade pending in the status strip to unlock ML auto-retrain.`,
      );
    }

    if (
      mlPhase3 != null &&
      mlPhase3.next_sidecar_category != null &&
      (mlPhase3.next_sidecar_samples_needed ?? 0) > 0
    ) {
      tips.unshift(
        `Phase 3 ML: grade more ${mlPhase3.next_sidecar_category} paper rows (${mlPhase3.next_sidecar_samples_needed} more) to unlock the next sidecar.`,
      );
    }

    return tips.slice(0, 4);
  }, [averageSpread, partialCatalog, mlPhase3]);

  const heroStats = [
    { label: 'Open market tape', value: marketCount || markets.length, detail: `${categoryCount || categories.length} categories` },
    { label: 'Visible liquidity', value: formatCompactMoney(visibleLiquidity), detail: `${formatCompactMoney(visibleVolume)} 24h volume` },
    {
      label: 'Average spread',
      value: averageSpread == null ? '-' : formatSpread(averageSpread),
      detail: partialCatalog ? 'Fast discovery mode' : cacheStatus === 'full' ? 'Full tape online' : 'Cache warming',
    },
  ];

  return (
    <div className="kalshiPage">
      <section className="kalshiHero">
        <div className="heroCopy">
          <p className="eyebrow">Kalshi command desk</p>
          <h1>Find the contract worth your next decision.</h1>
          <p>
            A Kalshi-first dashboard for market structure, paper portfolio discipline,
            analyst prompts, and high-signal opportunities across the live event tape.
          </p>
          <div className="heroActions">
            <button type="button" className="primaryBtn" onClick={() => void refreshAll()} disabled={refreshing || loading}>
              {refreshing ? 'Refreshing...' : 'Refresh and snapshot'}
            </button>
            <span className="muted small">
              {cacheAgeSecs != null ? `Cache age ${cacheAgeSecs}s` : 'Cache warming'}
              {lastRefreshAt ? ` - last refresh ${formatDateLabel(lastRefreshAt)}` : ''}
            </span>
          </div>
        </div>
        <div className="heroLedger" aria-label="Kalshi dashboard summary">
          {heroStats.map((stat) => (
            <div className="ledgerTile" key={stat.label}>
              <span>{stat.label}</span>
              <strong>{stat.value}</strong>
              <small>{stat.detail}</small>
            </div>
          ))}
        </div>
      </section>

      <div className="dashboardStatus" aria-label="Market data status">
        <span>{cacheLabel(cacheStatus, partialCatalog)}</span>
        {cacheAgeSecs != null && <span>Cache age {cacheAgeSecs}s</span>}
        {lastRefreshAt && <span>Last refresh {formatDateLabel(lastRefreshAt)}</span>}
        <span>Markets {marketCount}</span>
        <span>Categories {categoryCount}</span>
        {dataQualityNotes.map((note) => (
          <span key={note} className="diagnosticNote">{note}</span>
        ))}
        {mlPhase3 ? (
          <span className="diagnosticNote" title="Multi-category ML readiness (Settings for full detail)">
            {mlPhase3DashboardLabel(mlPhase3)}
          </span>
        ) : null}
        {mlPhase3 != null && mlPhase3.kalshi_pending_predictions > 0 ? (
          <button
            type="button"
            className="ghostBtn smallGradeBtn"
            disabled={gradingPending || loading}
            onClick={() => void gradePendingKalshi()}
            title="Resolve pending Kalshi paper rows against settled markets (may trigger ML auto-retrain)"
          >
            {gradingPending ? 'Grading…' : `Grade ${mlPhase3.kalshi_pending_predictions} pending`}
          </button>
        ) : null}
        {mlPhase3 != null && mlPhase3.auto_retrain_eligible ? (
          <button
            type="button"
            className="ghostBtn smallGradeBtn"
            disabled={mlTraining || loading}
            onClick={() => void trainMlFromDashboard()}
            title="Train unified + sidecar models (same as Settings ML card)"
          >
            {mlTraining ? 'Training ML…' : 'Train ML models'}
          </button>
        ) : null}
        {gradeFlash ? <span className="diagnosticNote">{gradeFlash}</span> : null}
        {mlTrainFlash ? <span className="diagnosticNote">{mlTrainFlash}</span> : null}
      </div>

      <section className="dashboardGrid">
        <article className="commandPanel">
          <div className="panelEyebrow">High-value shortlist</div>
          <div className="panelHeader">
            <h2>Best markets to inspect now</h2>
            <p className="muted">Ranked by liquidity, 24h activity, tradable price, and spread discipline.</p>
          </div>
          <div className="opportunityList">
            {highValueMarkets.map((market, index) => (
              <button
                key={market.ticker}
                type="button"
                className="opportunityCard"
                aria-label={`Open high-value market ${index + 1}`}
                onClick={() => setSelectedMarket(market)}
              >
                <span className="rankLabel">{String(index + 1).padStart(2, '0')}</span>
                <div>
                  <code>{market.category}</code>
                  <h3>{market.ticker}</h3>
                  <div className="marketStats">
                    <span>{marketBias(market)}</span>
                    <span>YES {formatProb(market.yes_prob_pct)}</span>
                    <span>Spread {formatSpread(market.spread)}</span>
                    <span>Liq {formatCompactMoney(market.liquidity)}</span>
                  </div>
                </div>
              </button>
            ))}
            {!loading && highValueMarkets.length === 0 && (
              <p className="muted pad">No open markets are ready for the shortlist yet.</p>
            )}
          </div>
        </article>

        <aside className="insightRail" aria-label="Dashboard guidance">
          <article className="insightCard accent">
            <span>Trading posture</span>
            <strong>{partialCatalog ? 'Refresh before size' : 'Full tape online'}</strong>
            <p>
              {partialCatalog
                ? 'Use this view for discovery, then refresh before committing a paper position.'
                : 'The catalog is ready for deeper analyst review and paper trade recording.'}
            </p>
          </article>
          <article className="insightCard">
            <span>Category pulse</span>
            {topCategories.length > 0 ? (
              <div className="categoryPulse">
                {topCategories.map((cat) => (
                  <div key={cat.category}>
                    <strong>{cat.category}</strong>
                    <small>{cat.count} markets - {formatCompactMoney(cat.volume_24h)} vol</small>
                  </div>
                ))}
              </div>
            ) : (
              <p className="muted small">Category stats will appear after the first market load.</p>
            )}
          </article>
          {mlPhase3 != null && (mlPhase3.non_sports_category_stats?.length ?? 0) > 0 ? (
            <article className="insightCard">
              <span>Sidecar data (Kalshi paper)</span>
              {mlPhase3.phase_3_data_metric_ready ? (
                <p className="muted small">ROADMAP data metric met — all three categories have enough graded rows for sidecars.</p>
              ) : null}
              {mlPhase3.unified_trained_at ? (
                <p className="muted small">Unified model trained {formatDateLabel(mlPhase3.unified_trained_at)}</p>
              ) : null}
              {mlPhase3.active_sidecar_models != null &&
              Object.keys(mlPhase3.active_sidecar_models).length > 0 ? (
                <p className="muted small">
                  Active sidecars:{' '}
                  {Object.entries(mlPhase3.active_sidecar_models)
                    .map(([name, m]) => {
                      const cv =
                        m.cv_accuracy_mean != null
                          ? `, CV ${(m.cv_accuracy_mean * 100).toFixed(1)}%`
                          : '';
                      return m.model_exists
                        ? `${name} (${m.samples} samples${cv})`
                        : `${name} (missing file)`;
                    })
                    .join(' · ')}
                </p>
              ) : null}
              <div className="categoryPulse">
                {mlPhase3.non_sports_category_stats!.map((row) => (
                  <div key={row.category}>
                    <strong>{row.category}</strong>
                    <small>
                      {row.resolved_count}/{row.min_resolved_for_sidecar} graded
                      {row.trainable ? ' · sidecar ready' : row.samples_until_trainable > 0 ? ` · +${row.samples_until_trainable} to unlock` : ''}
                    </small>
                  </div>
                ))}
              </div>
            </article>
          ) : null}
          <article className="insightCard">
            <span>Decision tips</span>
            <ul className="tipList">
              {dashboardTips.map((tip) => (
                <li key={tip}>{tip}</li>
              ))}
            </ul>
          </article>
        </aside>
      </section>

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
        <section className="marketTape">
          <div className="tapeHeader">
            <div>
              <p className="panelEyebrow">Market tape</p>
              <h2>Searchable Kalshi catalog</h2>
            </div>
            <span className="muted small">{markets.length} visible market{markets.length === 1 ? '' : 's'}</span>
          </div>
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
        </section>
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
