/**
 * Kalshi taker fee helpers for paper UX (mirror Rust edge_engine fee_per_contract).
 * Unrounded per-contract fee in dollars at price p (0–1).
 * Formula: fee_multiplier · p · (1 − p). Default multiplier 0.07.
 */

export const DEFAULT_FEE_MULTIPLIER = 0.07;

/** Per-contract fee in dollars (not rounded). */
export function feePerContractDollars(
  priceDollars: number,
  feeMultiplier: number = DEFAULT_FEE_MULTIPLIER,
): number {
  const p = Math.max(0, Math.min(1, priceDollars));
  return feeMultiplier * p * (1 - p);
}

/** Approximate total fee + stake for a paper lot sized in dollars at entry price. */
export function feePreviewForStake(
  stakeDollars: number,
  priceDollars: number,
  feeMultiplier: number = DEFAULT_FEE_MULTIPLIER,
): {
  contracts: number;
  feePerContract: number;
  totalFee: number;
  totalDebit: number;
  entryCostPerContract: number;
} {
  const p = Math.max(0.01, Math.min(0.99, priceDollars));
  const fee = feePerContractDollars(p, feeMultiplier);
  const entryCost = p + fee;
  // Stake is typically entry price * qty (cost basis); contracts ≈ stake / p
  const contracts = p > 0 ? stakeDollars / p : 0;
  const totalFee = fee * contracts;
  return {
    contracts,
    feePerContract: fee,
    totalFee,
    totalDebit: stakeDollars + totalFee,
    entryCostPerContract: entryCost,
  };
}

/** One-line UI string for TAKE ticket. */
export function formatFeePreviewLine(
  stakeDollars: number,
  priceDollars: number,
  feeMultiplier: number = DEFAULT_FEE_MULTIPLIER,
): string {
  if (!(stakeDollars > 0) || !(priceDollars > 0)) {
    return 'Fee preview: set a TAKE stake and entry price.';
  }
  const prev = feePreviewForStake(stakeDollars, priceDollars, feeMultiplier);
  return `Fee preview: ~$${prev.totalFee.toFixed(2)} taker fee on ~${prev.contracts.toFixed(0)} contracts @ $${priceDollars.toFixed(2)} (entry+fee ≈ $${prev.entryCostPerContract.toFixed(3)}/ctr).`;
}
