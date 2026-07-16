import { describe, expect, test } from 'vitest';
import {
  feePerContractDollars,
  feePreviewForStake,
  formatFeePreviewLine,
} from './kalshiFees';

describe('kalshiFees', () => {
  test('fee at 50c matches 0.07*0.5*0.5', () => {
    expect(feePerContractDollars(0.5)).toBeCloseTo(0.0175, 6);
  });

  test('fee preview scales with stake', () => {
    const a = feePreviewForStake(50, 0.5);
    const b = feePreviewForStake(100, 0.5);
    expect(b.totalFee).toBeCloseTo(a.totalFee * 2, 5);
    expect(a.contracts).toBeCloseTo(100, 5);
  });

  test('format line mentions fee', () => {
    const s = formatFeePreviewLine(25, 0.4);
    expect(s).toMatch(/Fee preview/i);
    expect(s).toMatch(/\$/);
  });
});
