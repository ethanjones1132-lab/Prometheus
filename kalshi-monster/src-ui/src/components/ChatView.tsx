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

export function ChatView() {
  const { messages, isStreaming, error, sendMessage, initSession } = useChat();
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    initSession();
  }, [initSession]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

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
};
