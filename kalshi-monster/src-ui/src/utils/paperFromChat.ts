import type { KalshiTradeDecision } from '../types/kalshi';

/**
 * Extract a paper-trade decision from an analyst message.
 * Prefers a fenced JSON object matching KalshiTradeDecision fields;
 * falls back to lightweight ticker / side / stake heuristics.
 */
export function extractPaperDecision(
  content: string,
  fallback?: { ticker?: string; title?: string; category?: string },
): KalshiTradeDecision | null {
  const fromJson = tryParseJsonDecision(content);
  if (fromJson) return fromJson;

  const ticker =
    content.match(/\b(KX[A-Z0-9][A-Z0-9\-]{4,})\b/i)?.[1]?.toUpperCase() ??
    fallback?.ticker;
  if (!ticker) return null;

  const sideMatch = content.match(
    /\b(?:contract[_\s-]?side|side|buy|recommend(?:ed|ation)?)\s*[:=]?\s*\*?\*?(YES|NO|PASS)\b/i,
  );
  const sideRaw = (sideMatch?.[1] ?? '').toUpperCase();
  if (sideRaw !== 'YES' && sideRaw !== 'NO' && sideRaw !== 'PASS') {
    // Require an explicit side for heuristic path — avoid mis-recording
    return null;
  }
  const contract_side = sideRaw as 'YES' | 'NO' | 'PASS';

  const fair =
    numFrom(content, /fair(?:\s+prob(?:ability)?)?[^0-9]{0,12}(\d{1,2}(?:\.\d+)?)\s*%/i) ??
    numFrom(content, /p_final[^0-9]{0,8}(0?\.\d+|1\.0)/i, true) ??
    numFrom(content, /model[^0-9]{0,12}(\d{1,2}(?:\.\d+)?)\s*%/i);
  const market =
    numFrom(content, /market(?:\s+price)?[^0-9]{0,12}(\d{1,2}(?:\.\d+)?)\s*%/i) ??
    numFrom(content, /p_market[^0-9]{0,8}(0?\.\d+|1\.0)/i, true) ??
    numFrom(content, /implied[^0-9]{0,12}(\d{1,2}(?:\.\d+)?)\s*%/i);
  const stake =
    numFrom(content, /stake[^$0-9]{0,8}\$?\s*(\d+(?:\.\d+)?)/i) ??
    numFrom(content, /\$(\d+(?:\.\d+)?)\s*(?:stake|kelly)/i) ??
    0;
  const entry =
    numFrom(content, /(?:enter|entry|ask)[^0-9]{0,10}(0?\.\d+|1\.0)/i, true) ??
    (market != null ? market / 100 : 0.5);

  const fairPct = fair != null ? (fair <= 1 ? fair * 100 : fair) : market ?? 50;
  const marketPct = market != null ? (market <= 1 ? market * 100 : market) : fairPct;
  const edge = fairPct - marketPct;
  const decision: KalshiTradeDecision['decision'] =
    contract_side === 'PASS' ? 'PASS' : stake > 0 && edge > 0 ? 'TAKE' : 'WATCH';

  return {
    ticker,
    market_title: fallback?.title ?? ticker,
    category: fallback?.category ?? 'Other',
    contract_side,
    market_price_pct: marketPct,
    fair_probability_pct: fairPct,
    edge_points: edge,
    spread_cents: 0,
    liquidity_score: 0,
    ev_per_contract_cents: edge,
    ev_roi_pct: marketPct > 0 ? (edge / marketPct) * 100 : 0,
    raw_kelly_pct: Math.max(0, edge * 2),
    fractional_kelly_pct: Math.max(0, edge * 0.5),
    recommended_stake_dollars: contract_side === 'PASS' ? 0 : stake,
    max_position_dollars: stake,
    decision,
    confidence_tier: decision === 'PASS' ? 'None' : edge > 5 ? 'High' : edge > 2 ? 'Medium' : 'Low',
    thesis: content.slice(0, 400),
    evidence: ['Extracted from Analyst reply'],
    risk_flags: [],
    data_quality: 'ChatExtract',
    price_to_enter: entry <= 1 ? entry * 100 : entry,
  };
}

function numFrom(text: string, re: RegExp, unitInterval = false): number | null {
  const m = text.match(re);
  if (!m) return null;
  const n = Number(m[1]);
  if (!Number.isFinite(n)) return null;
  if (unitInterval && n > 1) return null;
  return n;
}

function tryParseJsonDecision(content: string): KalshiTradeDecision | null {
  const fence = content.match(/```(?:json)?\s*([\s\S]*?)```/i);
  const candidates: string[] = [];
  if (fence?.[1]) candidates.push(fence[1].trim());
  // bare object with ticker
  const bare = content.match(/\{[\s\S]*?"ticker"\s*:\s*"[^"]+"[\s\S]*?\}/);
  if (bare) candidates.push(bare[0]);

  for (const raw of candidates) {
    try {
      const obj = JSON.parse(raw) as Record<string, unknown>;
      if (typeof obj.ticker !== 'string') continue;
      const side = String(obj.contract_side ?? obj.side ?? 'PASS').toUpperCase();
      if (side !== 'YES' && side !== 'NO' && side !== 'PASS') continue;
      const market_price_pct = Number(obj.market_price_pct ?? obj.market_price ?? 50);
      const fair_probability_pct = Number(obj.fair_probability_pct ?? obj.fair_probability ?? market_price_pct);
      const stake = Number(obj.recommended_stake_dollars ?? obj.stake ?? 0);
      const decision = String(obj.decision ?? (side === 'PASS' ? 'PASS' : stake > 0 ? 'TAKE' : 'WATCH')).toUpperCase();
      const d: KalshiTradeDecision = {
        ticker: obj.ticker,
        market_title: String(obj.market_title ?? obj.title ?? obj.ticker),
        category: String(obj.category ?? 'Other'),
        contract_side: side as 'YES' | 'NO' | 'PASS',
        market_price_pct,
        fair_probability_pct,
        edge_points: Number(obj.edge_points ?? fair_probability_pct - market_price_pct),
        spread_cents: Number(obj.spread_cents ?? 0),
        liquidity_score: Number(obj.liquidity_score ?? 0),
        ev_per_contract_cents: Number(obj.ev_per_contract_cents ?? 0),
        ev_roi_pct: Number(obj.ev_roi_pct ?? 0),
        raw_kelly_pct: Number(obj.raw_kelly_pct ?? 0),
        fractional_kelly_pct: Number(obj.fractional_kelly_pct ?? 0),
        recommended_stake_dollars: stake,
        max_position_dollars: Number(obj.max_position_dollars ?? stake),
        decision: (['TAKE', 'WATCH', 'PASS'].includes(decision) ? decision : 'WATCH') as
          | 'TAKE'
          | 'WATCH'
          | 'PASS',
        confidence_tier: (['High', 'Medium', 'Low', 'None'].includes(String(obj.confidence_tier))
          ? obj.confidence_tier
          : 'Medium') as KalshiTradeDecision['confidence_tier'],
        thesis: String(obj.thesis ?? ''),
        evidence: Array.isArray(obj.evidence) ? (obj.evidence as string[]) : [],
        risk_flags: Array.isArray(obj.risk_flags) ? (obj.risk_flags as string[]) : [],
        data_quality: String(obj.data_quality ?? 'ChatExtract'),
        price_to_enter: Number(obj.price_to_enter ?? market_price_pct),
      };
      return d;
    } catch {
      // try next candidate
    }
  }
  return null;
}
