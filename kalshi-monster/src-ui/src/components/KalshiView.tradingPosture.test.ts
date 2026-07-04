import { describe, expect, test } from 'vitest';
import { tradingPostureFromTape } from './KalshiView';

describe('tradingPostureFromTape', () => {
  test('in-progress refresh wins over stale and snapshot notes', () => {
    const notes = [
      'Market tape is older than 60s — use Refresh and snapshot for live prices',
      'Instant paint from saved market snapshot; live refresh runs in background',
      'Live catalog refresh in progress — tape may update shortly',
    ];
    expect(tradingPostureFromTape(false, 'full', notes)).toEqual({
      title: 'Catalog updating',
      body: 'Live refresh is running — wait for the tape to settle before sizing a paper trade.',
    });
  });

  test('stale tape when older-than-60s note without in-progress', () => {
    const notes = ['Market tape is older than 60s — use Refresh and snapshot for live prices'];
    expect(tradingPostureFromTape(false, 'full', notes)).toEqual({
      title: 'Stale tape',
      body: 'Prices are older than 60s — hit Refresh and snapshot before recording paper trades.',
    });
  });

  test('snapshot paint when persisted snapshot note without higher-priority hints', () => {
    const notes = ['Instant paint from saved market snapshot; live refresh runs in background'];
    expect(tradingPostureFromTape(false, 'quick', notes)).toEqual({
      title: 'Snapshot paint',
      body: 'You are seeing a saved snapshot for fast load — refresh once live data lands before committing size.',
    });
  });

  test('partial catalog discovery mode', () => {
    expect(tradingPostureFromTape(true, 'quick', ['Partial catalog loaded for fast first paint'])).toEqual({
      title: 'Refresh before size',
      body: 'Use this view for discovery, then refresh before committing a paper position.',
    });
  });

  test('full tape online when catalog complete and cache full', () => {
    expect(tradingPostureFromTape(false, 'full', ['Full catalog cache ready'])).toEqual({
      title: 'Full tape online',
      body: 'The catalog is ready for deeper analyst review and paper trade recording.',
    });
  });

  test('cache warming fallback', () => {
    expect(tradingPostureFromTape(false, 'quick', [])).toEqual({
      title: 'Cache warming',
      body: 'Market tape is still loading — refresh if the board looks thin.',
    });
  });
});