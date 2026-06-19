# Backtest Report — Kalshi Monster — market entry-price calibration + CLV (resolved markets)

## Caveats

- ⚠️ Data: 1008 settled Kalshi markets across series ["KXHIGHNY", "KXHIGHCHI", "KXHIGHAUS", "KXHIGHDEN", "KXHIGHLAX", "KXHIGHMIA", "KXFED", "KXCPIYOY", "KXPAYROLLS", "KXBTCD", "KXETHD"] (public API, volume >= 500, lifetime >= 6h), cached in eval-data/.
- ⚠️ Forecast under test: the market's ENTRY price (first traded candle) as P(yes). This measures market calibration — the benchmark the app's own forecasts must beat — and exercises CLV end to end (closing_prob = final candle). 'ROI' is the return of blindly buying YES at entry on every market; expect ~zero minus spread.
- ⚠️ The app's LLM forecasts have no graded history yet; once predictions.db accumulates resolved rows the same engine scores them via eval_adapter.

## Overall calibration

| metric | value |
|---|---|
| n (resolved) | 1008 (adequate) |
| wins / losses / pushes / voids | 236 / 772 / 0 / 0 |
| win rate | 23.4% CI [0.2073, 0.2589] |
| mean predicted prob | 0.2006 |
| Brier score | 0.1291 CI [0.1163, 0.1421] |
| Brier skill score | 0.2798 |
| log-loss | 0.4022 |
| ECE | 0.0371 CI [0.0273, 0.0609] |

### Reliability curve

| predicted range | n | mean predicted | empirical win rate | gap |
|---|---|---|---|---|
| 0.0–0.1 | 383 | 4.4% | 3.9% | -0.005 |
| 0.1–0.2 | 187 | 14.1% | 15.0% | +0.009 |
| 0.2–0.3 | 185 | 24.2% | 28.6% | +0.045 |
| 0.3–0.4 | 125 | 33.8% | 36.8% | +0.030 |
| 0.4–0.5 | 57 | 43.5% | 56.1% | +0.126 |
| 0.5–0.6 | 37 | 52.3% | 75.7% | +0.234 |
| 0.6–0.7 | 8 | 64.5% | 100.0% | +0.355 |
| 0.7–0.8 | 7 | 75.3% | 100.0% | +0.247 |
| 0.8–0.9 | 7 | 85.1% | 100.0% | +0.149 |
| 0.9–1.0 | 12 | 95.5% | 100.0% | +0.045 |

## Realized returns (flat replay)

| metric | value |
|---|---|
| decided bets | 1008 |
| refunded (push/void) | 0 |
| total staked (units) | 1008.00 |
| net P/L (units) | +48.01 |
| ROI | 4.8% |
| avg units/bet | 0.0476 |
| max drawdown (units) | 204.78 |
| log-growth/bet (1u = 1% roll) | -0.0000 |
| CLV (n=1008) | mean +0.0297, positive 22.8% |

## By category

| segment | n | sufficiency | win rate | mean pred | Brier | ECE | ROI |
|---|---|---|---|---|---|---|---|
| KXHIGHAUS | 150 | THIN | 16.7% | 0.1679 | 0.1207 | 0.0322 | 1.9% |
| KXHIGHCHI | 150 | THIN | 16.7% | 0.1687 | 0.1195 | 0.0271 | -6.3% |
| KXHIGHDEN | 150 | THIN | 16.7% | 0.1716 | 0.1153 | 0.0495 | -26.1% |
| KXHIGHLAX | 150 | THIN | 17.3% | 0.1767 | 0.1084 | 0.0458 | -32.1% |
| KXHIGHMIA | 150 | THIN | 19.3% | 0.1874 | 0.1229 | 0.0510 | 16.5% |
| KXHIGHNY | 150 | THIN | 17.3% | 0.1763 | 0.1097 | 0.0643 | -30.3% |
| KXCPIYOY | 61 | THIN | 72.1% | 0.3589 | 0.2839 | 0.3625 | 213.4% |
| KXPAYROLLS | 36 | INSUFFICIENT | 88.9% | 0.5353 | 0.2202 | 0.3608 | 104.9% |
| KXFED | 11 | INSUFFICIENT | 36.4% | 0.3373 | 0.0427 | 0.1391 | -48.3% |

## By edge bucket

| segment | n | sufficiency | win rate | mean pred | Brier | ECE | ROI |
|---|---|---|---|---|---|---|---|
| longshot <20c | 570 | adequate | 7.5% | 0.0756 | 0.0663 | 0.0061 | -7.0% |
| 20-40c | 310 | adequate | 31.9% | 0.2808 | 0.2161 | 0.0386 | 14.8% |
| 40-60c | 94 | THIN | 63.8% | 0.4700 | 0.2529 | 0.1683 | 35.4% |
| favorite >80c | 19 | INSUFFICIENT | 100.0% | 0.9168 | 0.0099 | 0.0832 | 9.5% |
| 60-80c | 15 | INSUFFICIENT | 100.0% | 0.6953 | 0.0962 | 0.3047 | 44.8% |

## Out-of-sample recalibration check

Calibrator: **isotonic** (fitted on first 70% = 705 preds; evaluated on remaining 303).

| metric | raw (test) | recalibrated (test) |
|---|---|---|
| Brier | 0.1450 | 0.1431 |
| log-loss | 0.4478 | 0.4442 |
| ECE | 0.0530 | 0.0482 |

