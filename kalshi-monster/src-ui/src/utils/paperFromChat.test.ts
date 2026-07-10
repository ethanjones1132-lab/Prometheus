import { describe, expect, test } from 'vitest';
import { extractPaperDecision } from './paperFromChat';

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
});
