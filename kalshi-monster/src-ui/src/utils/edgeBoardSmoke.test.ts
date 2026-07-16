/**
 * Lightweight E2E-style smoke for Edge Board → paper decision construction.
 * (Full Playwright deferred; this guards the pure decision mapping.)
 */
import { describe, expect, test } from 'vitest';
import { formatFeePreviewLine } from './kalshiFees';
import { sanitizeDecisionUnitsAndCaps } from './paperFromChat';
import type { KalshiTradeDecision } from '../types/kalshi';

function edgeToDecision(
  ticker: string,
  side: 'YES' | 'NO',
  pMarket: number,
  pFinal: number,
  stake: number,
): KalshiTradeDecision {
  const market_pct = (side === 'YES' ? pMarket : 1 - pMarket) * 100;
  const fair_pct = (side === 'YES' ? pFinal : 1 - pFinal) * 100;
  return sanitizeDecisionUnitsAndCaps({
    ticker,
    market_title: ticker,
    category: 'Financials',
    contract_side: side,
    market_price_pct: market_pct,
    fair_probability_pct: fair_pct,
    edge_points: fair_pct - market_pct,
    spread_cents: 1,
    liquidity_score: 50,
    ev_per_contract_cents: fair_pct - market_pct,
    ev_roi_pct: 10,
    raw_kelly_pct: 5,
    fractional_kelly_pct: 1.25,
    recommended_stake_dollars: stake,
    max_position_dollars: stake,
    decision: 'TAKE',
    confidence_tier: 'Medium',
    thesis: 'Edge Board smoke',
    evidence: [],
    risk_flags: [],
    data_quality: 'Live',
    price_to_enter: side === 'YES' ? pMarket : 1 - pMarket,
    model_disagreement: false,
  });
}

describe('Edge Board → paper smoke', () => {
  test('builds TAKE with agent fair and fee preview', () => {
    const d = edgeToDecision('KXBTCD-TEST', 'YES', 0.4, 0.52, 25);
    expect(d.decision).toBe('TAKE');
    expect(d.fair_probability_pct).toBeCloseTo(52, 0);
    expect(d.recommended_stake_dollars).toBeGreaterThan(0);
    const fee = formatFeePreviewLine(d.recommended_stake_dollars, d.price_to_enter);
    expect(fee).toMatch(/Fee preview/i);
  });

  test('flags disagreement when LLM fair far from agent p_final', () => {
    const d = sanitizeDecisionUnitsAndCaps(
      {
        ticker: 'KXTEST-1A',
        market_title: 'T',
        category: 'Politics',
        contract_side: 'YES',
        market_price_pct: 40,
        fair_probability_pct: 70,
        edge_points: 30,
        spread_cents: 1,
        liquidity_score: 50,
        ev_per_contract_cents: 30,
        ev_roi_pct: 75,
        raw_kelly_pct: 40,
        fractional_kelly_pct: 5,
        recommended_stake_dollars: 20,
        max_position_dollars: 20,
        decision: 'TAKE',
        confidence_tier: 'High',
        thesis: 'LLM edge',
        evidence: [],
        risk_flags: [],
        data_quality: 'Live',
        price_to_enter: 0.4,
      },
      { agentPFinalPct: 45, bankrollDollars: 10000, maxBetPct: 0.05 },
    );
    expect(d.model_disagreement).toBe(true);
    expect(d.risk_flags).toContain('ModelDisagreement');
  });
});
