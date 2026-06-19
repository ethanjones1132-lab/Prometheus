import { useCallback, useEffect, useState } from 'react';
import { kalshiApi } from '../services/kalshi';
import type {
  KalshiMarketSummary,
  KalshiPriceHistory,
  KalshiTradeDecision,
  StakeAdjustment,
} from '../types/kalshi';
import { PriceHistoryChart } from './PriceHistoryChart';

interface Props {
  market: KalshiMarketSummary;
  onClose: () => void;
}

export function MarketDetailPanel({ market, onClose }: Props) {
  const [contractSide, setContractSide] = useState<'YES' | 'NO'>('YES');
  const [fairProb, setFairProb] = useState(market.yes_prob_pct);
  const [rawStake, setRawStake] = useState(25);
  const [adjustment, setAdjustment] = useState<StakeAdjustment | null>(null);
  const [history, setHistory] = useState<KalshiPriceHistory | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const marketPricePct =
    contractSide === 'YES' ? market.yes_prob_pct : 100 - market.yes_prob_pct;
  const edgePts = fairProb - marketPricePct;

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
    void loadAdjustment();
  }, [loadAdjustment]);

  useEffect(() => {
    setHistoryLoading(true);
    kalshiApi
      .getPriceHistory(market.ticker, 120)
      .then(setHistory)
      .catch(() => setHistory(null))
      .finally(() => setHistoryLoading(false));
  }, [market.ticker]);

  const recordPaperTrade = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const stake = adjustment?.adjusted_recommended_stake ?? rawStake;
      const decision: KalshiTradeDecision = {
        ticker: market.ticker,
        market_title: market.title,
        category: market.category,
        contract_side: contractSide,
        market_price_pct: marketPricePct,
        fair_probability_pct: fairProb,
        edge_points: edgePts,
        spread_cents: market.spread * 100,
        liquidity_score: Math.min(100, market.liquidity / 500),
        ev_per_contract_cents: edgePts,
        ev_roi_pct: marketPricePct > 0 ? (edgePts / marketPricePct) * 100 : 0,
        raw_kelly_pct: Math.max(0, edgePts * 2),
        fractional_kelly_pct: Math.max(0, edgePts * 0.5),
        recommended_stake_dollars: stake,
        max_position_dollars: stake,
        decision: edgePts > 2 ? 'TAKE' : edgePts > 0 ? 'WATCH' : 'PASS',
        confidence_tier: edgePts > 5 ? 'High' : edgePts > 2 ? 'Medium' : 'Low',
        thesis: `Paper trade on ${market.ticker} with portfolio-adjusted Kelly.`,
        evidence: [`Market ${marketPricePct.toFixed(1)}% vs fair ${fairProb.toFixed(1)}%`],
        risk_flags: adjustment && adjustment.kelly_scale < 1 ? ['CorrelatedExposure'] : [],
        data_quality: 'Live',
        price_to_enter: contractSide === 'YES' ? market.yes_ask : market.no_ask,
      };
      const id = await kalshiApi.recordPaperDecision('paper-sim', decision);
      setMessage(`Paper trade recorded (${id.slice(0, 8)}…)`);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modalBackdrop" onClick={onClose}>
      <div className="modalPanel" onClick={(e) => e.stopPropagation()}>
        <header className="modalHeader">
          <div>
            <code>{market.ticker}</code>
            <h3>{market.title}</h3>
            <span className="muted">{market.category} · spread {(market.spread * 100).toFixed(1)}¢</span>
          </div>
          <button type="button" className="ghostBtn" onClick={onClose}>
            Close
          </button>
        </header>

        <section className="modalSection">
          <h4>Price history</h4>
          <PriceHistoryChart history={history} loading={historyLoading} />
        </section>

        <section className="modalSection tradeTicket">
          <h4>Trade ticket (P1 risk-adjusted)</h4>
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
            <label>Fair prob {fairProb.toFixed(1)}%</label>
            <input
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
              max={500}
              value={rawStake}
              onChange={(e) => setRawStake(Number(e.target.value))}
            />
          </div>

          {adjustment && (
            <div className="adjustmentBox">
              <div className="adjustmentHead">
                <span>
                  Kelly scale <strong>{(adjustment.kelly_scale * 100).toFixed(0)}%</strong>
                </span>
                <span>
                  Adjusted stake{' '}
                  <strong>${adjustment.adjusted_recommended_stake.toFixed(2)}</strong>
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

          <button type="button" className="primaryBtn" disabled={busy} onClick={recordPaperTrade}>
            Record paper trade
          </button>
          {message && <p className="muted small">{message}</p>}
        </section>
      </div>
    </div>
  );
}