import type { KalshiTradeDecision } from '../types/kalshi';

/** Default app policy mirrors bankroll.rs (quarter Kelly, 5% max bet). */
export const DEFAULT_KELLY_FRACTION = 0.25;
export const DEFAULT_MAX_BET_PCT = 0.05;

/**
 * Coerce a contract price that may be dollars (0–1), cents, or percent into dollars [0, 1].
 */
export function coercePriceToDollars(raw: number): number {
  if (!Number.isFinite(raw) || raw <= 0) return 0;
  if (raw <= 1) return raw;
  if (raw <= 100) return raw / 100;
  return Math.min(1, raw / 100);
}

/**
 * Joint normalize so sub-1% markets (market_price_pct=0.45 meaning 0.45%,
 * price_to_enter=0.005 dollars) are not misread as $0.45.
 * Returns [market_price_pct 0–100, price_to_enter 0–1].
 */
export function coerceMarketAndEntry(
  marketPricePct: number,
  priceToEnter: number,
): [number, number] {
  const enter = priceToEnter > 0 ? coercePriceToDollars(priceToEnter) : 0;
  if (enter > 0 && enter <= 1) {
    if (marketPricePct > 1) return [Math.min(100, Math.max(0, marketPricePct)), enter];
    if (marketPricePct <= 0) return [Math.min(100, enter * 100), enter];
    const rel = Math.abs(marketPricePct - enter) / Math.max(enter, 1e-9);
    if (rel < 0.25) return [Math.min(100, marketPricePct * 100), enter];
    return [Math.min(100, Math.max(0, marketPricePct)), enter];
  }
  const marketDollars = coercePriceToDollars(marketPricePct);
  const enter2 = enter > 0 ? enter : marketDollars;
  return [Math.min(100, marketDollars * 100), Math.min(1, Math.max(0, enter2))];
}

/**
 * Coerce a probability that may be 0–1 or 0–100 into percent [0, 100].
 */
export function coerceProbabilityToPct(raw: number): number {
  if (!Number.isFinite(raw)) return 50;
  if (raw < 0) return 0;
  if (raw <= 1) return raw * 100;
  return Math.min(100, raw);
}

export type SizingPolicy = {
  bankrollDollars?: number;
  kellyFraction?: number;
  maxBetPct?: number;
  /** Optional agent p_final (0–100 pct) for ModelDisagreement vs LLM fair. */
  agentPFinalPct?: number;
};

/**
 * Normalize price units and cap Kelly/stake before a TAKE is shown or papered.
 * - market_price_pct → 0–100 (% of $1)
 * - fair_probability_pct → 0–100
 * - price_to_enter → 0–1 dollars
 * - fractional_kelly_pct ≤ maxBetPct * 100 (and ≤ raw * kellyFraction when raw known)
 */
export function sanitizeDecisionUnitsAndCaps(
  decision: KalshiTradeDecision,
  policy: SizingPolicy = {},
): KalshiTradeDecision {
  const kellyFraction = policy.kellyFraction ?? DEFAULT_KELLY_FRACTION;
  const maxBetPct = policy.maxBetPct ?? DEFAULT_MAX_BET_PCT;
  const bankroll = policy.bankrollDollars ?? 0;

  const [market_price_pct, price_to_enter] = coerceMarketAndEntry(
    decision.market_price_pct,
    decision.price_to_enter,
  );
  let fair_probability_pct = coerceProbabilityToPct(decision.fair_probability_pct);

  let raw_kelly_pct = Math.max(0, decision.raw_kelly_pct);
  let fractional_kelly_pct = Math.max(0, decision.fractional_kelly_pct);
  let recommended_stake_dollars = Math.max(0, decision.recommended_stake_dollars);
  let max_position_dollars = Math.max(0, decision.max_position_dollars);
  const risk_flags = [...(decision.risk_flags ?? [])];
  let thesis = decision.thesis ?? '';

  if (raw_kelly_pct > 0) {
    const policyFrac = raw_kelly_pct * kellyFraction;
    if (fractional_kelly_pct <= 0 || fractional_kelly_pct > policyFrac + 0.01) {
      fractional_kelly_pct = policyFrac;
    }
  }

  const maxFracKellyPct = maxBetPct * 100;
  let capped = false;
  if (fractional_kelly_pct > maxFracKellyPct) {
    fractional_kelly_pct = maxFracKellyPct;
    capped = true;
  }
  if (fractional_kelly_pct > 25) {
    fractional_kelly_pct = Math.min(25, maxFracKellyPct);
    capped = true;
  }

  if (bankroll > 0) {
    const maxStake = bankroll * maxBetPct;
    if (
      recommended_stake_dollars <= 0 &&
      decision.decision === 'TAKE' &&
      fractional_kelly_pct > 0
    ) {
      recommended_stake_dollars = bankroll * (fractional_kelly_pct / 100);
    }
    if (recommended_stake_dollars > maxStake) {
      recommended_stake_dollars = maxStake;
      capped = true;
    }
    max_position_dollars =
      max_position_dollars > 0
        ? Math.min(max_position_dollars, maxStake, Math.max(recommended_stake_dollars, 0))
        : Math.min(maxStake, recommended_stake_dollars);
  } else if (fractional_kelly_pct > maxFracKellyPct) {
    // No bankroll known — still clamp the displayed Kelly fraction
    fractional_kelly_pct = maxFracKellyPct;
    capped = true;
  }

  if (capped) {
    if (!risk_flags.includes('BankrollLimitExceeded')) {
      risk_flags.push('BankrollLimitExceeded');
    }
    if (decision.decision === 'TAKE' && !thesis.includes('[sizing capped')) {
      const note = `[sizing capped: fractional Kelly ≤ ${maxFracKellyPct.toFixed(1)}% bankroll]`;
      thesis = thesis ? `${thesis} ${note}` : note;
    }
  }

  // Recompute edge display consistency when units were mixed
  const edge_points =
    Number.isFinite(decision.edge_points) && decision.edge_points !== 0
      ? decision.edge_points
      : fair_probability_pct - market_price_pct;

  // Always emit model_disagreement so Tauri IPC never rejects the ticket
  // (Rust field is #[serde(default)] but we still send an explicit bool).
  let model_disagreement =
    typeof decision.model_disagreement === 'boolean'
      ? decision.model_disagreement
      : Math.abs(fair_probability_pct - market_price_pct) >= 15;

  // Gap fix: force ModelDisagreement when LLM fair vs agent p_final diverges ≥10pts.
  const agentFinalPct = policy.agentPFinalPct;
  if (
    agentFinalPct != null &&
    Number.isFinite(agentFinalPct) &&
    Math.abs(fair_probability_pct - agentFinalPct) >= 10
  ) {
    model_disagreement = true;
    if (!risk_flags.includes('ModelDisagreement')) {
      risk_flags.push('ModelDisagreement');
    }
    if (!thesis.includes('[agent disagreement')) {
      thesis = `${thesis} [agent disagreement: LLM fair ${fair_probability_pct.toFixed(1)}% vs p_final ${agentFinalPct.toFixed(1)}%]`.trim();
    }
  }

  return {
    ...decision,
    market_price_pct,
    fair_probability_pct,
    price_to_enter,
    edge_points,
    raw_kelly_pct,
    fractional_kelly_pct,
    recommended_stake_dollars,
    max_position_dollars,
    risk_flags,
    thesis,
    model_disagreement,
  };
}

/** Schema placeholders the backend also rejects (must never paper-trade). */
export function isPlaceholderTicker(ticker: string): boolean {
  const t = (ticker || '').trim().toUpperCase();
  if (!t) return true;
  if (
    t === 'KXEVENT-TICKER' ||
    t === 'KX-EVENT-TICKER' ||
    t === 'KXEVENT' ||
    t === 'TICKER' ||
    t === 'KX-TICKER' ||
    t === 'KXTEST' ||
    t.endsWith('-TICKER') ||
    t.includes('PLACEHOLDER') ||
    t.includes('EXAMPLE')
  ) {
    return true;
  }
  if (!t.startsWith('KX') || !t.includes('-') || t.length < 6) return true;
  return false;
}

/**
 * Prefer the JSON deliverable portion of a free-model monologue (UI mirror of
 * backend prefer_deliverable_content — presentation only).
 */
export function preferDeliverableContent(text: string): string {
  const t = (text || '').trim();
  if (!t) return '';
  const jsonIdx = t.indexOf('```json');
  if (jsonIdx >= 0 && t.length - jsonIdx >= 80) return t.slice(jsonIdx).trim();
  const fenceIdx = t.indexOf('```');
  if (fenceIdx >= 0) {
    const body = t.slice(fenceIdx + 3).trimStart();
    if (body.startsWith('{') && body.includes('"ticker"') && t.length - fenceIdx >= 80) {
      return t.slice(fenceIdx).trim();
    }
  }
  const brace = t.indexOf('{');
  if (
    brace >= 0 &&
    t.includes('"ticker"') &&
    t.includes('"decision"') &&
    (brace > 400 || /^thinking/i.test(t) || t.includes('Analyze the Request'))
  ) {
    return t.slice(brace).trim();
  }
  return t;
}

/**
 * Policy rails mirroring backend enforce_prediction_quality_rails (no math changes).
 */
export function enforcePredictionQualityRails(d: KalshiTradeDecision): KalshiTradeDecision {
  const risk_flags = [...(d.risk_flags ?? [])];
  let thesis = d.thesis ?? '';
  let decision = d.decision;
  let confidence_tier = d.confidence_tier;
  let recommended_stake_dollars = d.recommended_stake_dollars;
  let max_position_dollars = d.max_position_dollars;
  let fractional_kelly_pct = d.fractional_kelly_pct;
  let raw_kelly_pct = d.raw_kelly_pct;
  let contract_side = d.contract_side;

  if (isPlaceholderTicker(d.ticker)) {
    return {
      ...d,
      decision: 'PASS',
      contract_side: 'PASS',
      recommended_stake_dollars: 0,
      max_position_dollars: 0,
      fractional_kelly_pct: 0,
      raw_kelly_pct: 0,
      confidence_tier: 'None',
      risk_flags: risk_flags.includes('StaleData') ? risk_flags : [...risk_flags, 'StaleData'],
      thesis: thesis.includes('[quality rail: placeholder')
        ? thesis
        : `${thesis} [quality rail: placeholder/invalid ticker — forced PASS]`.trim(),
    };
  }

  if (
    confidence_tier === 'High' &&
    ['Inferential', 'Speculative', 'Stale'].includes(String(d.data_quality))
  ) {
    confidence_tier = 'Low';
    if (!thesis.includes('[quality rail: High confidence')) {
      thesis = `${thesis} [quality rail: High confidence requires Live/Fresh data — demoted]`.trim();
    }
  }

  const absEdge = Math.abs(d.edge_points ?? 0);
  if (d.spread_cents > 0 && absEdge > 0 && d.spread_cents > absEdge) {
    if (!risk_flags.includes('SpreadExceedsEdge')) risk_flags.push('SpreadExceedsEdge');
    if (decision === 'TAKE') {
      decision = 'PASS';
      recommended_stake_dollars = 0;
      max_position_dollars = 0;
      fractional_kelly_pct = 0;
      raw_kelly_pct = 0;
      confidence_tier = 'None';
      if (!thesis.includes('[quality rail: spread exceeds')) {
        thesis = `${thesis} [quality rail: spread exceeds edge — forced PASS]`.trim();
      }
    }
  }

  const mkt = d.market_price_pct;
  const fair = d.fair_probability_pct;
  if (mkt > 0 && mkt < 5 && fair > mkt * 5) {
    if (!risk_flags.includes('ExtremeProbability')) risk_flags.push('ExtremeProbability');
    if (!risk_flags.includes('ModelDisagreement')) risk_flags.push('ModelDisagreement');
    const weak = ['Inferential', 'Speculative', 'Stale'].includes(String(d.data_quality));
    if (decision === 'TAKE' && weak) {
      decision = 'WATCH';
      recommended_stake_dollars = 0;
      max_position_dollars = 0;
      fractional_kelly_pct = 0;
      if (confidence_tier === 'High' || confidence_tier === 'Medium') confidence_tier = 'Low';
      if (!thesis.includes('[quality rail: longshot')) {
        thesis =
          `${thesis} [quality rail: longshot fair≫market without Live data — demoted to WATCH]`.trim();
      }
    }
  }

  if (decision === 'TAKE' && d.spread_cents >= 25 && (d.liquidity_score ?? 0) < 20) {
    if (!risk_flags.includes('InsufficientLiquidity')) risk_flags.push('InsufficientLiquidity');
    decision = 'PASS';
    recommended_stake_dollars = 0;
    max_position_dollars = 0;
    fractional_kelly_pct = 0;
    raw_kelly_pct = 0;
    confidence_tier = 'None';
    if (!thesis.includes('[quality rail: wide illiquid')) {
      thesis = `${thesis} [quality rail: wide illiquid book — forced PASS]`.trim();
    }
  }

  return {
    ...d,
    decision,
    contract_side,
    confidence_tier,
    recommended_stake_dollars,
    max_position_dollars,
    fractional_kelly_pct,
    raw_kelly_pct,
    risk_flags,
    thesis,
  };
}

/**
 * Extract a paper-trade decision from an analyst message.
 * Prefers a fenced JSON object matching KalshiTradeDecision fields;
 * falls back to lightweight ticker / side / stake heuristics.
 * Always runs unit normalization + Kelly caps before return.
 */
export function extractPaperDecision(
  content: string,
  fallback?: { ticker?: string; title?: string; category?: string },
  policy?: SizingPolicy,
): KalshiTradeDecision | null {
  const cleaned = preferDeliverableContent(content);
  const fromJson = tryParseJsonDecision(cleaned);
  if (fromJson) {
    if (isPlaceholderTicker(fromJson.ticker)) return null;
    return enforcePredictionQualityRails(sanitizeDecisionUnitsAndCaps(fromJson, policy));
  }

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
    (market != null ? (market <= 1 ? market : market / 100) : 0.5);

  const fairPct = fair != null ? (fair <= 1 ? fair * 100 : fair) : market ?? 50;
  const marketPct = market != null ? (market <= 1 ? market * 100 : market) : fairPct;
  const edge = fairPct - marketPct;
  const decision: KalshiTradeDecision['decision'] =
    contract_side === 'PASS' ? 'PASS' : stake > 0 && edge > 0 ? 'TAKE' : 'WATCH';

  const raw: KalshiTradeDecision = {
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
    price_to_enter: entry <= 1 ? entry : entry / 100,
  };
  return sanitizeDecisionUnitsAndCaps(raw, policy);
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
  const candidates: string[] = [];
  const fenceRe = /```(?:json)?\s*([\s\S]*?)```/gi;
  let m: RegExpExecArray | null;
  while ((m = fenceRe.exec(content)) !== null) {
    if (m[1]?.trim()) candidates.push(m[1].trim());
  }
  // bare object with ticker (greedy enough for multi-line)
  const bare = content.match(/\{[\s\S]*?"ticker"\s*:\s*"[^"]+"[\s\S]*?\}/);
  if (bare) candidates.push(bare[0]);

  // Prefer last valid candidate (refined revision after monologue).
  let last: KalshiTradeDecision | null = null;
  for (const raw of candidates) {
    try {
      const cleaned = raw.replace(/,\s*([}\]])/g, '$1');
      const obj = JSON.parse(cleaned) as Record<string, unknown>;
      if (typeof obj.ticker !== 'string') continue;
      if (isPlaceholderTicker(obj.ticker)) continue;
      const side = String(obj.contract_side ?? obj.side ?? 'PASS').toUpperCase();
      if (side !== 'YES' && side !== 'NO' && side !== 'PASS') continue;
      const market_price_pct = Number(obj.market_price_pct ?? obj.market_price ?? 50);
      const fair_probability_pct = Number(
        obj.fair_probability_pct ?? obj.fair_probability ?? market_price_pct,
      );
      const stake = Number(obj.recommended_stake_dollars ?? obj.stake ?? 0);
      const decision = String(
        obj.decision ?? (side === 'PASS' ? 'PASS' : stake > 0 ? 'TAKE' : 'WATCH'),
      ).toUpperCase();
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
        model_disagreement:
          typeof obj.model_disagreement === 'boolean'
            ? obj.model_disagreement
            : Math.abs(fair_probability_pct - market_price_pct) >= 15,
      };
      last = d;
    } catch {
      // try next candidate
    }
  }
  return last;
}
