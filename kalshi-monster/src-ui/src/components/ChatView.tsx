import { useState, useEffect, useRef } from 'react';
import { useChat } from '../hooks/useChat';
import type { ChatMessage } from '../types';

const QUICK_PROMPTS = [
  { label: 'Mispriced Markets', query: 'What are the most mispriced markets on Kalshi today? Analyze implied probabilities vs expected values.' },
  { label: 'Fed Rate Analysis', query: 'Analyze the latest Fed rate decision contracts on Kalshi. What do the markets predict?' },
  { label: 'Election Edge', query: 'Which election markets on Kalshi currently have the best edge? Consider volume, spread, and recent movement.' },
  { label: 'Crypto Predictions', query: 'What are the top crypto price predictions on Kalshi? Analyze BTC and ETH contracts.' },
  { label: 'High Volume', query: 'Show me the highest volume markets on Kalshi with favorable odds. What trends do you see?' },
  { label: 'Weather Markets', query: 'Are there any weather-related prediction markets on Kalshi? What do they indicate?' },
  { label: 'Economic Indicators', query: 'What economic indicator markets (CPI, GDP, unemployment) are available on Kalshi? Any mispricings?' },
  { label: 'Parlay Opportunities', query: 'Identify potential parlay opportunities on Kalshi with uncorrelated or positively correlated outcomes.' },
];

interface ChatViewProps {
  initialPrompt?: string | null;
  onPromptConsumed?: () => void;
}

function extractTickerFromPrompt(prompt: string): { ticker: string; title: string } | null {
  const m = prompt.match(/Analyze Kalshi market (\S+): (.+?)($|\n|Category:)/);
  if (!m) return null;
  return { ticker: m[1], title: m[2] };
}

export function ChatView({ initialPrompt, onPromptConsumed }: ChatViewProps = {}) {
  const { messages, isStreaming, error, sendMessage, initSession, kalshiContextStatus } = useChat();
  const [input, setInput] = useState('');
  const [activeContext, setActiveContext] = useState<{ ticker: string; title: string } | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    initSession();
  }, [initSession]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    if (!initialPrompt) return;
    setInput(initialPrompt);
    const ctx = extractTickerFromPrompt(initialPrompt);
    if (ctx) setActiveContext(ctx);
    onPromptConsumed?.();
  }, [initialPrompt]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || isStreaming) return;
    sendMessage(input.trim());
    setInput('');
  };

  const handleQuickPrompt = (query: string) => {
    if (isStreaming) return;
    sendMessage(query);
  };

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>🧠 Kalshi Intelligence</h2>
        <span style={styles.subtitle}>AI-Powered Market Analysis</span>
      </div>

            {/* Active Market Context */}
      {activeContext && (
        <div style={styles.contextChip}>
          <div style={styles.contextChipInner}>
            <span style={styles.contextBadge}>🔍 {activeContext.ticker}</span>
            <span style={styles.contextTitle}>{activeContext.title}</span>
            <button
              style={styles.contextDismiss}
              onClick={() => setActiveContext(null)}
              title="Dismiss context"
            >
              ×
            </button>
          </div>
          <div style={styles.contextHint}>
            The AI sees live Kalshi market data. Responses factor in current prices, volume, and category trends.
          </div>
        </div>
      )}

      {/* Degraded Kalshi tape (KB-2a) */}
      {kalshiContextStatus?.degraded && (
        <div style={styles.degradedBanner} role="alert">
          <strong>⚠️ Limited Kalshi market context</strong>
          {kalshiContextStatus.reasons.length > 0 ? (
            <ul style={styles.degradedList}>
              {kalshiContextStatus.reasons.map((reason) => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          ) : (
            <span> Market tape has {kalshiContextStatus.tape_market_count} markets — refresh the catalog on Markets.</span>
          )}
        </div>
      )}

      {/* Legacy hint when a ticker is pinned but tape looks cold before first message */}
      {activeContext && messages.length === 0 && !isStreaming && kalshiContextStatus?.degraded && (
        <div style={styles.contextHintOnly}>
          Pinned market: responses may omit live tape until you refresh the catalog.
        </div>
      )}

{/* Quick Prompts */}
      <div style={styles.quickPrompts}>
        {QUICK_PROMPTS.map((prompt) => (
          <button
            key={prompt.label}
            style={styles.quickPromptBtn}
            onClick={() => handleQuickPrompt(prompt.query)}
            disabled={isStreaming}
          >
            {prompt.label}
          </button>
        ))}
      </div>

      {/* Messages */}
      <div style={styles.messages}>
        {messages.length === 0 && (
          <div style={styles.emptyState}>
            <p>Ask about Kalshi markets, get AI-powered analysis</p>
            <p style={styles.hint}>Try a quick prompt above or type your own question</p>
          </div>
        )}
        {messages.map((msg) => (
          <MessageBubble key={msg.id} message={msg} />
        ))}
        {isStreaming && (
          <div style={styles.streaming}>
            <span style={styles.dot}>●</span>
            <span style={styles.dot}>●</span>
            <span style={styles.dot}>●</span>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Error */}
      {error && (
        <div style={styles.error}>
          ⚠️ {error}
        </div>
      )}

      {/* Input */}
      <form style={styles.inputBar} onSubmit={handleSubmit}>
        <input
          style={styles.input}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Ask about prediction markets..."
          disabled={isStreaming}
        />
        <button
          type="submit"
          style={styles.sendBtn}
          disabled={isStreaming || !input.trim()}
        >
          Send
        </button>
      </form>
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  return (
    <div style={{
      ...styles.messageBubble,
      ...(isUser ? styles.userBubble : styles.assistantBubble),
    }}>
      {message.reasoning && (
        <details style={styles.reasoning}>
          <summary>💭 Reasoning</summary>
          <p>{message.reasoning}</p>
        </details>
      )}
      <div style={styles.messageContent}>{message.content}</div>
      {message.tokens_used != null && (
        <span style={styles.tokens}>{message.tokens_used} tokens</span>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: 'flex',
    flexDirection: 'column',
    height: '100%',
    background: '#0d1117',
    color: '#c9d1d9',
  },
  header: {
    padding: '16px 20px',
    borderBottom: '1px solid #30363d',
  },
  title: {
    margin: 0,
    fontSize: '18px',
    color: '#58a6ff',
  },
  subtitle: {
    fontSize: '12px',
    color: '#8b949e',
  },
  quickPrompts: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: '8px',
    padding: '12px 16px',
    borderBottom: '1px solid #30363d',
  },
  quickPromptBtn: {
    padding: '6px 12px',
    borderRadius: '16px',
    border: '1px solid #30363d',
    background: '#161b22',
    color: '#c9d1d9',
    fontSize: '12px',
    cursor: 'pointer',
  },
  messages: {
    flex: 1,
    overflowY: 'auto',
    padding: '16px',
    display: 'flex',
    flexDirection: 'column',
    gap: '12px',
  },
  emptyState: {
    textAlign: 'center' as const,
    color: '#8b949e',
    marginTop: '40px',
  },
  hint: {
    fontSize: '13px',
    color: '#6e7681',
  },
  messageBubble: {
    maxWidth: '80%',
    padding: '12px 16px',
    borderRadius: '12px',
    fontSize: '14px',
    lineHeight: '1.5',
  },
  userBubble: {
    alignSelf: 'flex-end',
    background: '#238636',
    color: '#fff',
  },
  assistantBubble: {
    alignSelf: 'flex-start',
    background: '#161b22',
    border: '1px solid #30363d',
  },
  reasoning: {
    marginBottom: '8px',
    padding: '8px',
    background: '#0d1117',
    borderRadius: '8px',
    fontSize: '12px',
    color: '#8b949e',
  },
  messageContent: {
    whiteSpace: 'pre-wrap',
  },
  tokens: {
    display: 'block',
    marginTop: '4px',
    fontSize: '10px',
    color: '#6e7681',
  },
  streaming: {
    display: 'flex',
    gap: '4px',
    padding: '12px 16px',
    alignSelf: 'flex-start',
  },
  dot: {
    color: '#58a6ff',
    animation: 'pulse 1.5s infinite',
  },
  error: {
    padding: '8px 16px',
    background: '#3f1518',
    color: '#f85149',
    fontSize: '13px',
    borderTop: '1px solid #30363d',
  },
  inputBar: {
    display: 'flex',
    gap: '8px',
    padding: '12px 16px',
    borderTop: '1px solid #30363d',
  },
  input: {
    flex: 1,
    padding: '10px 14px',
    borderRadius: '8px',
    border: '1px solid #30363d',
    background: '#0d1117',
    color: '#c9d1d9',
    fontSize: '14px',
    outline: 'none',
  },
  sendBtn: {
    padding: '10px 20px',
    borderRadius: '8px',
    border: 'none',
    background: '#238636',
    color: '#fff',
    fontSize: '14px',
    cursor: 'pointer',
  },
  contextChip: {
    margin: '12px 16px 0',
    padding: '10px 14px',
    background: '#1a2332',
    border: '1px solid #1f6feb',
    borderRadius: '10px',
  },
  contextChipInner: {
    display: 'flex',
    alignItems: 'center',
    gap: '10px',
  },
  contextBadge: {
    padding: '2px 8px',
    background: '#1f6feb',
    color: '#fff',
    borderRadius: '4px',
    fontSize: '12px',
    fontWeight: 600,
    fontFamily: 'monospace',
  },
  contextTitle: {
    flex: 1,
    fontSize: '13px',
    color: '#c9d1d9',
    fontWeight: 500,
  },
  contextDismiss: {
    padding: '0 4px',
    background: 'none',
    border: 'none',
    color: '#8b949e',
    fontSize: '16px',
    cursor: 'pointer',
    lineHeight: 1,
  },
  contextHint: {
    marginTop: '6px',
    fontSize: '11px',
    color: '#6e7681',
  },
  degradedBanner: {
    margin: '8px 16px 0',
    padding: '8px 12px',
    background: '#3d2e00',
    border: '1px solid #d29922',
    borderRadius: '6px',
    fontSize: '12px',
    color: '#e3b341',
  },
  degradedList: {
    margin: '6px 0 0',
    paddingLeft: '18px',
  },
  contextHintOnly: {
    margin: '4px 16px 0',
    fontSize: '11px',
    color: '#8b949e',
  },
};
