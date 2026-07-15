import { useCallback, useEffect, useState } from 'react';
import { bankrollApi, configApi } from '../services/tauri';
import type { AppConfig, BankrollConfig } from '../types';
import { kalshiApi } from '../services/kalshi';
import type {
  KalshiMarketSummary,
  KalshiPriceHistory,
  KalshiTradeDecision,
  CalibrationStatus,
  StakeAdjustment,
} from '../types/kalshi';
import { PriceHistoryChart } from './PriceHistoryChart';

interface Props {
  market: KalshiMarketSummary;
  onClose: () => void;
  onAnalyzeMarket?: (prompt: string) => void;
}

function formatDateLabel(value?: string | null): string {
  if (!value) return 'Not listed';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Invalid date';
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    timeZone: 'UTC',
    timeZoneName: 'short',
  }).format(date);
}

function formatMoney(value: number): string {
  return `$${value.toLocaleString(undefined, { maximumFractionDigits: 0 })}`;
}

function sideMarketPrice(market: KalshiMarketSummary, side: 'YES' | 'NO'): number {
  return side === 'YES' ? market.yes_prob_pct : 100 - market.yes_prob_pct;
}

function sideFairProbability(fairYes: number, side: 'YES' | 'NO'): number {
  return side === 'YES' ? fairYes : 100 - fairYes;
}

function sideAsk(market: KalshiMarketSummary, side: 'YES' | 'NO'): number {
  return side === 'YES' ? market.yes_ask : market.no_ask;
}

export function MarketDetailPanel({ market, onClose, onAnalyzeMarket }: Props) {
  const [contractSide, setContractSide] = useState<'YES' | 'NO'>('YES');
  const [fairProb, setFairProb] = useState(market.yes_prob_pct);
  const [rawStake, setRawStake] = useState(25);
  const [adjustment, setAdjustment] = useState<StakeAdjustment | null>(null);
  const [calibration, setCalibration] = useState<CalibrationStatus | null>(null);
  const [history, setHistory] = useState<KalshiPriceHistory | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [edgeBusy, setEdgeBusy] = useState(false);
  const [edgeSummary, setEdgeSummary] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [bankrollConfig, setBankrollConfig] = useState<BankrollConfig | null>(null);
  const [stakeConfigLoading, setStakeConfigLoading] = useState(true);
  const [stakeConfigError, setStakeConfigError] = useState<string | null>(null);

  const marketPricePct = sideMarketPrice(market, contractSide);
  const selectedFairPct = sideFairProbability(fairProb, contractSide);
  const edgePts = selectedFairPct - marketPricePct;
  const selectedPrice = sideAsk(market, contractSide);
  // Human labels for UI chips; `code` is the Rust RiskFlag variant name for paper IPC.
  const riskFlagRows = (
    [
      market.can_close_early ? { label: 'Early close', code: 'EarlyCloseRisk' } : null,
      market.is_provisional ? { label: 'Provisional', code: 'ProvisionalSettlement' } : null,
      market.spread * 100 > Math.max(edgePts, 0)
        ? { label: 'Spread exceeds edge', code: 'SpreadExceedsEdge' }
        : null,
      market.liquidity <= 0
        ? { label: 'No visible liquidity', code: 'InsufficientLiquidity' }
        : null,
    ] as ({ label: string; code: string } | null)[]
  ).filter((x): x is { label: string; code: string } => x != null);
  const riskFlags = riskFlagRows.map((r) => r.label);
  const maxStakeDollars = Math.max(
    1,
    Math.round(((bankrollConfig?.total_bankroll ?? 1000) * (config?.max_bet_pct ?? 0.05)) * 100) / 100,
  );
  const canRecordSelected = edgePts > 0 && rawStake > 0 && selectedPrice > 0;

  useEffect(() => {
    let cancelled = false;
    Promise.all([configApi.get(), bankrollApi.getConfig()])
      .then(([appConfig, bankroll]) => {
        if (cancelled) return;
        setConfig(appConfig);
        setBankrollConfig(bankroll);
        const maxStake = Math.max(1, bankroll.total_bankroll * appConfig.max_bet_pct);
        setRawStake((current) => Math.min(Math.max(1, current), maxStake));
      })
      .catch((e) => {
        if (!cancelled) {
          setStakeConfigError(e instanceof Error ? e.message : String(e));
        }
      })
      .finally(() => {
        if (!cancelled) setStakeConfigLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const updateMaxBetPct = async (value: number) => {
    if (!config || !bankrollConfig) return;
    const nextPct = Math.max(0.1, Math.min(25, value));
    const nextConfig: AppConfig = {
      ...config,
      max_bet_pct: nextPct / 100,
    };
    const nextMaxStakeDollars = Math.max(
      1,
      Math.round(((bankrollConfig.total_bankroll * nextPct) / 100) * 100) / 100,
    );
    setConfig(nextConfig);
    setRawStake((current) => Math.min(Math.max(1, current), nextMaxStakeDollars));
    try {
      await configApi.save(nextConfig);
    } catch (e) {
      setConfig(config);
      setStakeConfigError(e instanceof Error ? e.message : String(e));
    }
  };

  const updateRawStake = (value: number) => {
    setRawStake(Math.max(1, Math.min(value, maxStakeDollars)));
  };

  const loadAdjustment = useCallback(async () => {
    try {
      const adj = await kalshiApi.computeStakeAdjustment({
        ticker: market.ticker,
        category: market.category,
        contractSide,
        recommendedStake: rawStake,
      });
      setAdjustment(adj);
    } catch {
      setAdjustment(null);
    }
  }, [market.ticker, market.category, contractSide, rawStake]);

  useEffect(() => {
    const handle = window.setTimeout(() => {
      void loadAdjustment();
    }, 300);
    return () => window.clearTimeout(handle);
  }, [loadAdjustment]);

  useEffect(() => {
    let cancelled = false;
    kalshiApi
      .getCalibrationStatus(fairProb)
      .then((status) => {
        if (!cancelled) setCalibration(status);
      })
      .catch(() => {
        if (!cancelled) setCalibration(null);
      });
    return () => {
      cancelled = true;
    };
  }, [fairProb]);

  useEffect(() => {
    setHistoryLoading(true);
    kalshiApi
      .getPriceHistory(market.ticker, 120)
      .then(setHistory)
      .catch(() => setHistory(null))
      .finally(() => setHistoryLoading(false));
  }, [market.ticker]);

  const buildDecision = (side: 'YES' | 'NO', action: 'TAKE' | 'WATCH' | 'PASS'): KalshiTradeDecision => {
    const pricePct = sideMarketPrice(market, side);
    const fairPct = sideFairProbability(fairProb, side);
    const edge = fairPct - pricePct;
    const stake = action === 'TAKE' ? (adjustment?.adjusted_recommended_stake ?? rawStake) : 0;
    return {
      ticker: market.ticker,
      market_title: market.title,
      category: market.category,
      contract_side: action === 'PASS' ? 'PASS' : side,
      market_price_pct: pricePct,
      fair_probability_pct: fairPct,
      edge_points: edge,
      spread_cents: market.spread * 100,
      liquidity_score: Math.min(100, market.liquidity / 500),
      ev_per_contract_cents: edge,
      ev_roi_pct: pricePct > 0 ? (edge / pricePct) * 100 : 0,
      raw_kelly_pct: action === 'TAKE' ? Math.max(0, edge * 2) : 0,
      fractional_kelly_pct: action === 'TAKE' ? Math.max(0, edge * 0.5) : 0,
      recommended_stake_dollars: stake,
      max_position_dollars: stake,
      decision: action,
      confidence_tier: action === 'PASS' ? 'None' : edge > 5 ? 'High' : edge > 2 ? 'Medium' : 'Low',
      thesis:
        action === 'PASS'
          ? `Pass logged on ${market.ticker}; edge is not actionable.`
          : action === 'WATCH'
            ? `Watch ${market.ticker}; wait for a better entry or fresher data.`
            : `Paper trade on ${market.ticker} with portfolio-adjusted Kelly.`,
      evidence: [
        `${side} market ${pricePct.toFixed(1)}% vs fair ${fairPct.toFixed(1)}%`,
        `Spread ${(market.spread * 100).toFixed(1)}c, liquidity ${formatMoney(market.liquidity)}`,
      ],
      risk_flags: [
        ...(adjustment && adjustment.kelly_scale < 1 ? ['CorrelatedExposure'] : []),
        ...riskFlagRows.map((r) => r.code),
      ],
      data_quality: 'Live',
      price_to_enter: sideAsk(market, side),
      model_disagreement: Math.abs(fairPct - pricePct) >= 15,
    };
  };

  const buildAnalystPrompt = () => {
    const selectedSidePrice = sideMarketPrice(market, contractSide);
    const selectedSideFair = sideFairProbability(fairProb, contractSide);
    return [
      `Analyze Kalshi market ${market.ticker}: ${market.title}`,
      `Category: ${market.category}; status: ${market.status}; close: ${formatDateLabel(market.close_time)}.`,
      `${contractSide} side is priced at ${selectedSidePrice.toFixed(1)}% with my fair probability at ${selectedSideFair.toFixed(1)}%, implying ${edgePts.toFixed(1)} points of edge.`,
      `Liquidity is ${formatMoney(market.liquidity)}, 24h volume is ${formatMoney(market.volume_24h)}, and spread is ${(market.spread * 100).toFixed(1)}c.`,
      riskFlags.length > 0 ? `Risk flags: ${riskFlags.join(', ')}.` : 'No blocking market-mechanics flags are visible.',
      'Give me a concise thesis, what could break it, whether to record YES/NO/watch/pass, and the risk-managed stake posture.',
    ].join('\n');
  };

  const recordPaperTrade = async (side: 'YES' | 'NO', action: 'TAKE' | 'WATCH' | 'PASS') => {
    setBusy(true);
    setMessage(null);
    try {
      const decision = buildDecision(side, action);
      const id = await kalshiApi.recordPaperDecision('paper-sim', decision);
      setMessage(`${action === 'TAKE' ? 'Paper trade' : action} recorded (${id.slice(0, 8)}...)`);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const runEdgeEngine = async () => {
    setEdgeBusy(true);
    setEdgeSummary(null);
    setMessage(null);
    try {
      const r = await kalshiApi.analyzeMarketEdge(market.ticker);
      const model =
        r.p_model == null ? 'n/a' : `${(r.p_model * 100).toFixed(1)}%`;
      setEdgeSummary(
        `Ledger #${r.forecast_id}: p_mkt ${(r.p_market * 100).toFixed(1)}% · p_model ${model} · p_final ${(r.p_final * 100).toFixed(1)}% · ${r.verdict} (${r.signals_opining}/${r.signals_received} agents)`,
      );
      setMessage(`Edge engine logged forecast #${r.forecast_id} (${r.verdict}). See Calibration tab.`);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setEdgeBusy(false);
    }
  };

  return (
    <div className="modalBackdrop" onClick={onClose}>
      <div className="modalPanel" onClick={(e) => e.stopPropagation()}>
        <header className="modalHeader">
          <div>
            <code>{market.ticker}</code>
            <h3>{market.title}</h3>
            <span className="muted">{market.category} - spread {(market.spread * 100).toFixed(1)}c</span>
          </div>
          <button type="button" className="primaryBtn" onClick={() => void runEdgeEngine()} disabled={edgeBusy}>
            {edgeBusy ? 'Running edge…' : 'Run edge engine'}
          </button>
          {onAnalyzeMarket && (
            <button type="button" className="primaryBtn" onClick={() => onAnalyzeMarket(buildAnalystPrompt())}>
              Analyze with AI
            </button>
          )}
          <button type="button" className="ghostBtn" onClick={onClose}>
            Close
          </button>
        </header>
        {edgeSummary && (
          <p className="muted" style={{ margin: '0 1rem 0.5rem' }}>
            {edgeSummary}
          </p>
        )}

        <section className="modalSection">
          <h4>Price history</h4>
          <PriceHistoryChart history={history} loading={historyLoading} />
        </section>

        <section className="modalSection">
          <h4>Market mechanics</h4>
          <div className="mechanicsGrid">
            <div>
              <span>Status</span>
              <strong>{market.status}</strong>
            </div>
            <div>
              <span>Close</span>
              <strong>{formatDateLabel(market.close_time)}</strong>
            </div>
            <div>
              <span>Expiration</span>
              <strong>{formatDateLabel(market.expiration_time)}</strong>
            </div>
            <div>
              <span>Liquidity</span>
              <strong>{formatMoney(market.liquidity)}</strong>
            </div>
          </div>
          <div className="riskFlagRow">
            {riskFlags.length > 0 ? (
              riskFlags.map((flag) => (
                <span key={flag} className="warnTag">
                  {flag}
                </span>
              ))
            ) : (
              <span className="statusPill ok">No blocking mechanics flags</span>
            )}
          </div>
        </section>

        <section className="modalSection tradeTicket">
          <h4>Decision ticket</h4>
          <div className="ticketRow">
            <label>Side</label>
            <div className="segControl">
              <button
                type="button"
                className={contractSide === 'YES' ? 'active' : ''}
                onClick={() => setContractSide('YES')}
              >
                YES
              </button>
              <button
                type="button"
                className={contractSide === 'NO' ? 'active' : ''}
                onClick={() => setContractSide('NO')}
              >
                NO
              </button>
            </div>
          </div>
          <div className="ticketRow">
            <label htmlFor="fair-probability">Fair probability {fairProb.toFixed(1)}%</label>
            <input
              id="fair-probability"
              aria-label="Fair probability"
              type="range"
              min={5}
              max={95}
              value={fairProb}
              onChange={(e) => setFairProb(Number(e.target.value))}
            />
          </div>
          <div className="ticketRow">
            <label>Market price</label>
            <strong>{marketPricePct.toFixed(1)}%</strong>
          </div>
          <div className="ticketRow">
            <label>Edge</label>
            <strong className={edgePts >= 0 ? 'pos' : 'neg'}>{edgePts.toFixed(1)} pts</strong>
          </div>
          <div className="ticketRow">
            <label>Raw Kelly stake $</label>
            <input
              type="number"
              min={1}
              max={maxStakeDollars}
              value={rawStake}
              onChange={(e) => updateRawStake(Number(e.target.value))}
            />
          </div>
          <div className="ticketRow">
            <label>Local max bet % bankroll</label>
            <input
              type="number"
              min={0.1}
              max={25}
              step={0.1}
              disabled={stakeConfigLoading}
              value={config ? (config.max_bet_pct * 100).toFixed(1) : '5.0'}
              onChange={(e) => void updateMaxBetPct(Number(e.target.value))}
            />
          </div>
          <div className="ticketRow">
            <label>Configured max stake</label>
            <strong>
              {stakeConfigLoading ? 'Loading...' : stakeConfigError ? 'Unavailable' : `$${maxStakeDollars.toFixed(2)}`}
            </strong>
          </div>
          {stakeConfigError && <p className="muted small">{stakeConfigError}</p>}

          {calibration && (
            <div className="calibrationBox">
              <div>
                <h5>Calibration & ML</h5>
                <span className="muted small">
                  {calibration.artifact_kind} / {calibration.source}
                </span>
              </div>
              <div className="calibrationGrid">
                <span>Raw {calibration.raw_pct.toFixed(1)}%</span>
                <span>Calibrated {calibration.calibrated_pct.toFixed(1)}%</span>
                <span className={calibration.adjustment_pct >= 0 ? 'pos' : 'neg'}>
                  Adjustment {calibration.adjustment_pct.toFixed(1)} pts
                </span>
                <span>Volatility haircut {calibration.volatility_haircut_pct.toFixed(1)}%</span>
                <span>{calibration.category_sample_status}</span>
                <span>n={calibration.n_fit.toLocaleString()}</span>
              </div>
            </div>
          )}

          {adjustment && (
            <div className="adjustmentBox">
              <div className="adjustmentHead">
                <span>
                  Kelly scale <strong>{(adjustment.kelly_scale * 100).toFixed(0)}%</strong>
                </span>
                <span>
                  Adjusted stake <strong>${adjustment.adjusted_recommended_stake.toFixed(2)}</strong>
                </span>
              </div>
              {adjustment.conflicts.map((c) => (
                <div key={c.exposure_ticker} className="conflictRow">
                  <span className="warnTag">{c.strength}</span>
                  <span>{c.exposure_ticker}</span>
                  <span className="muted">{c.explanation}</span>
                </div>
              ))}
              {adjustment.warnings.map((w) => (
                <p key={w} className="warnText">
                  {w}
                </p>
              ))}
            </div>
          )}

          {!canRecordSelected && (
            <p className="warnText">Recording is disabled until this side has positive edge and a valid price.</p>
          )}
          <div className="decisionActions">
            <button
              type="button"
              className="primaryBtn"
              disabled={busy || !(fairProb - market.yes_prob_pct > 0) || market.yes_ask <= 0}
              onClick={() => void recordPaperTrade('YES', 'TAKE')}
            >
              Record YES
            </button>
            <button
              type="button"
              className="primaryBtn"
              disabled={busy || !((100 - fairProb) - (100 - market.yes_prob_pct) > 0) || market.no_ask <= 0}
              onClick={() => void recordPaperTrade('NO', 'TAKE')}
            >
              Record NO
            </button>
            <button
              type="button"
              className="ghostBtn"
              disabled={busy}
              onClick={() => void recordPaperTrade(contractSide, 'WATCH')}
            >
              Watch
            </button>
            <button
              type="button"
              className="ghostBtn"
              disabled={busy}
              onClick={() => void recordPaperTrade(contractSide, 'PASS')}
            >
              Pass
            </button>
          </div>
          {message && <p className="muted small">{message}</p>}
        </section>
      </div>
    </div>
  );
}
