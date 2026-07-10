import { describe, expect, test } from 'vitest';
import {
  coerceMarketAndEntry,
  coercePriceToDollars,
  coerceProbabilityToPct,
  extractPaperDecision,
  sanitizeDecisionUnitsAndCaps,
} from './paperFromChat';
import type { KalshiTradeDecision } from '../types/kalshi';

describe('extractPaperDecision', () => {
  test('parses fenced JSON decision', () => {
    const content = `
Here is my call:
\`\`\`json
{
  "ticker": "KXTEST-1",
  "market_title": "Test market",
  "category": "Economics",
  "contract_side": "YES",
  "market_price_pct": 40,
  "fair_probability_pct": 55,
  "recommended_stake_dollars": 25,
  "decision": "TAKE",
  "price_to_enter": 42
}
\`\`\`
`;
    const d = extractPaperDecision(content);
    expect(d).not.toBeNull();
    expect(d!.ticker).toBe('KXTEST-1');
    expect(d!.contract_side).toBe('YES');
    expect(d!.recommended_stake_dollars).toBe(25);
    expect(d!.decision).toBe('TAKE');
    // price_to_enter normalized from cents → dollars
    expect(d!.price_to_enter).toBeCloseTo(0.42);
    expect(d!.market_price_pct).toBeCloseTo(40);
  });

  test('parses heuristic YES with fair and market pct', () => {
    const content =
      'On KXFED-SEP I recommend side: YES. Market price 48%. Fair probability 58%. Stake $15.';
    const d = extractPaperDecision(content);
    expect(d).not.toBeNull();
    expect(d!.ticker).toBe('KXFED-SEP');
    expect(d!.contract_side).toBe('YES');
    expect(d!.fair_probability_pct).toBeCloseTo(58);
    expect(d!.market_price_pct).toBeCloseTo(48);
  });

  test('returns null without ticker or side', () => {
    expect(extractPaperDecision('Markets look interesting today.')).toBeNull();
  });

  test('caps absurd fractional Kelly before TAKE papering', () => {
    const content = `
\`\`\`json
{
  "ticker": "KXHILTON-CA",
  "market_title": "Hilton CA jungle",
  "category": "Politics",
  "contract_side": "YES",
  "market_price_pct": 0.1,
  "fair_probability_pct": 40,
  "raw_kelly_pct": 99.8,
  "fractional_kelly_pct": 99.8,
  "recommended_stake_dollars": 9980,
  "decision": "TAKE",
  "price_to_enter": 0.1
}
\`\`\`
`;
    const d = extractPaperDecision(content, undefined, { bankrollDollars: 10_000 });
    expect(d).not.toBeNull();
    expect(d!.market_price_pct).toBeCloseTo(10);
    expect(d!.price_to_enter).toBeCloseTo(0.1);
    expect(d!.fractional_kelly_pct).toBeLessThanOrEqual(5.001);
    expect(d!.recommended_stake_dollars).toBeLessThanOrEqual(500.001);
    expect(d!.risk_flags).toContain('BankrollLimitExceeded');
  });
});

describe('sanitizeDecisionUnitsAndCaps', () => {
  const base = (): KalshiTradeDecision => ({
    ticker: 'KX-T',
    market_title: 'T',
    category: 'Other',
    contract_side: 'YES',
    market_price_pct: 55,
    fair_probability_pct: 60,
    edge_points: 5,
    spread_cents: 1,
    liquidity_score: 50,
    ev_per_contract_cents: 5,
    ev_roi_pct: 9,
    raw_kelly_pct: 20,
    fractional_kelly_pct: 5,
    recommended_stake_dollars: 50,
    max_position_dollars: 50,
    decision: 'TAKE',
    confidence_tier: 'Medium',
    thesis: '',
    evidence: [],
    risk_flags: [],
    data_quality: 'Live',
    price_to_enter: 0.55,
  });

  test('coerce helpers', () => {
    expect(coercePriceToDollars(0.55)).toBeCloseTo(0.55);
    expect(coercePriceToDollars(55)).toBeCloseTo(0.55);
    expect(coerceProbabilityToPct(0.62)).toBeCloseTo(62);
    expect(coerceProbabilityToPct(62)).toBeCloseTo(62);
  });

  test('normalizes dollar market input to percent', () => {
    const d = sanitizeDecisionUnitsAndCaps({
      ...base(),
      market_price_pct: 0.55,
      price_to_enter: 0.55,
    });
    expect(d.market_price_pct).toBeCloseTo(55);
    expect(d.price_to_enter).toBeCloseTo(0.55);
  });

  test('preserves sub-1% market with dollar entry', () => {
    const [pct, enter] = coerceMarketAndEntry(0.45, 0.005);
    expect(pct).toBeCloseTo(0.45);
    expect(enter).toBeCloseTo(0.005);
    const d = sanitizeDecisionUnitsAndCaps({
      ...base(),
      market_price_pct: 0.45,
      price_to_enter: 0.005,
      fair_probability_pct: 2,
      raw_kelly_pct: 1.5,
      fractional_kelly_pct: 0.38,
      recommended_stake_dollars: 50,
    });
    expect(d.market_price_pct).toBeCloseTo(0.45);
    expect(d.price_to_enter).toBeCloseTo(0.005);
  });
});
