import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { ChatView } from './ChatView';

const sendMessage = vi.fn();
const initSession = vi.fn();
const refreshSessions = vi.fn();
const selectSession = vi.fn();
const deleteSession = vi.fn();

vi.mock('../hooks/useChat', () => ({
  useChat: () => ({
    messages: [],
    sessions: [
      {
        id: 's1',
        name: 'Fed thread',
        created_at: '2026-07-10T00:00:00Z',
        updated_at: '2026-07-10T01:00:00Z',
        model: 'test',
        message_count: 2,
        total_tokens: 100,
      },
    ],
    sessionId: 's1',
    isStreaming: false,
    streamingText: '',
    streamingThought: '',
    error: null,
    lastFailedPrompt: null,
    sendMessage,
    initSession,
    selectSession,
    deleteSession,
    refreshSessions,
    cancelStream: vi.fn(),
    retryLast: vi.fn(),
    clearError: vi.fn(),
    kalshiContextStatus: {
      degraded: true,
      tape_market_count: 0,
      reasons: ['No markets loaded'],
    },
    refreshKalshiContextStatus: vi.fn(),
  }),
}));

vi.mock('../services/kalshi', () => ({
  kalshiApi: {
    getCategoryStats: vi.fn().mockResolvedValue([]),
    recordPaperDecision: vi.fn(),
    analyzeTopMarketsEdge: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../services/tauri', () => ({
  finceptApi: {
    getBridgeStatus: vi.fn().mockResolvedValue({
      online: true,
      degraded: false,
      restarts_remaining: 3,
    }),
  },
}));

describe('ChatView Analyst UX', () => {
  beforeEach(() => {
    sendMessage.mockReset();
    initSession.mockResolvedValue('s1');
    refreshSessions.mockResolvedValue(undefined);
  });

  test('shows sessions, degraded banner, and markets CTA', async () => {
    const onOpenMarkets = vi.fn();
    render(<ChatView onOpenMarkets={onOpenMarkets} />);

    expect(await screen.findByLabelText('Analyst workspace')).toBeInTheDocument();
    expect(screen.getByText('Fed thread')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent(/Limited Kalshi market context/i);
    expect(screen.getByText('No markets loaded')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Open Command desk/i }));
    expect(onOpenMarkets).toHaveBeenCalled();
  });

  test('pins market context from initialPrompt', async () => {
    render(
      <ChatView
        initialPrompt={'Analyze Kalshi market KXFED-SEP: Will the Fed cut?\nCategory: Economics'}
        onPromptConsumed={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('KXFED-SEP')).toBeInTheDocument();
    });
    expect(screen.getByRole('code')).toHaveTextContent('KXFED-SEP');
    // title appears in context chip <strong>
    expect(document.querySelector('.analystContextChip strong')?.textContent).toMatch(/Will the Fed cut/i);
  });
});
