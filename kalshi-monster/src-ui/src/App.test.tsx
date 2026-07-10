import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, test, vi } from 'vitest';
import App from './App';

vi.mock('./components/KalshiView', () => ({
  KalshiView: ({ onAnalyzeMarket }: { onAnalyzeMarket: (prompt: string) => void }) => (
    <main aria-label="Kalshi markets surface">
      Kalshi markets surface
      <button type="button" onClick={() => onAnalyzeMarket('Analyze KX-FED with liquidity and edge context')}>
        Analyze selected market
      </button>
    </main>
  ),
}));

vi.mock('./components/ChatView', () => ({
  ChatView: ({
    initialPrompt,
    onOpenMarkets,
  }: {
    initialPrompt?: string | null;
    onOpenMarkets?: () => void;
  }) => (
    <main aria-label="Analyst workspace">
      Analyst chat surface
      {initialPrompt && <p>{initialPrompt}</p>}
      {onOpenMarkets && (
        <button type="button" onClick={onOpenMarkets}>
          Open Command desk &amp; refresh tape
        </button>
      )}
    </main>
  ),
}));

vi.mock('./components/KalshiPredictionsPanel', () => ({
  KalshiPredictionsPanel: () => <section>Paper trades surface</section>,
}));

vi.mock('./components/SettingsView', () => ({
  SettingsView: () => <main>Settings surface</main>,
}));

vi.mock('./components/CalibrationView', () => ({
  CalibrationView: () => <main aria-label="Calibration surface">Calibration surface</main>,
}));

vi.mock('./components/WorldMarketsView', () => ({
  WorldMarketsView: () => <main>World markets surface</main>,
}));

describe('App shell', () => {
  test('opens to the Kalshi command desk and keeps prop tooling out of primary navigation', () => {
    render(<App />);

    expect(screen.getByLabelText('Kalshi markets surface')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Command desk' })).toHaveClass('active');
    expect(screen.queryByRole('button', { name: /prop board/i })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Calibration' })).toBeInTheDocument();
  });

  test('opens the Calibration tab', () => {
    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: 'Calibration' }));
    expect(screen.getByLabelText('Calibration surface')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Calibration' })).toHaveClass('active');
  });

  test('hands a selected market analysis prompt to the analyst workspace', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Analyze selected market' }));

    expect(screen.getByText('Analyst chat surface')).toBeInTheDocument();
    expect(screen.getByText('Analyze KX-FED with liquidity and edge context')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Analyst' })).toHaveClass('active');
  });
});
