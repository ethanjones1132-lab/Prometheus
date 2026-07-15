/** Fired after Analyst or Markets records a paper decision so portfolio panels refresh. */
export const KALSHI_PAPER_UPDATED = 'kalshi-paper-updated';

export function notifyPaperUpdated(): void {
  window.dispatchEvent(new CustomEvent(KALSHI_PAPER_UPDATED));
}