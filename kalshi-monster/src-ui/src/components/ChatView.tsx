import { useState, useEffect, useRef, useMemo } from 'react';
import { useChat } from '../hooks/useChat';
import { kalshiApi } from '../services/kalshi';
import type { ChatMessage } from '../types';
import type { KalshiCategoryStat } from '../types/kalshi';
import { extractPaperDecision } from '../utils/paperFromChat';
import { formatChatText } from '../utils/formatChatText';

const FALLBACK_PROMPTS = [
  {
    label: 'Mispriced markets',
    query:
      'What are the most mispriced markets on Kalshi today? Compare implied probabilities to a careful fair value.',
  },
  {
    label: 'Fed / rates',
    query: 'Analyze the latest Fed / rate decision contracts on Kalshi. What does the market price imply?',
  },
  {
    label: 'High volume',
    query: 'Show the highest-volume open Kalshi markets and where the book looks thin vs liquid.',
  },
  {
    label: 'Economic releases',
    query: 'Which CPI, GDP, or unemployment contracts look interesting this week? Flag catalysts.',
  },
];

interface ChatViewProps {
  initialPrompt?: string | null;
  onPromptConsumed?: () => void;
  /** Switch shell to Command desk when tape is cold (KB-1 path). */
  onOpenMarkets?: () => void;
  /** Switch to Paper portfolio after a successful paper record. */
  onOpenPaper?: () => void;
}

function extractTickerFromPrompt(prompt: string): { ticker: string; title: string } | null {
  const m = prompt.match(/Analyze Kalshi market (\S+): (.+?)($|\n|Category:)/);
  if (!m) return null;
  return { ticker: m[1], title: m[2] };
}

export function ChatView({
  initialPrompt,
  onPromptConsumed,
  onOpenMarkets,
  onOpenPaper,
}: ChatViewProps = {}) {
  const {
    messages,
    sessions,
    sessionId,
    isStreaming,
    streamingText,
    streamingThought,
    error,
    lastFailedPrompt,
    sendMessage,
    initSession,
    selectSession,
    deleteSession,
    refreshSessions,
    cancelStream,
    retryLast,
    clearError,
    kalshiContextStatus,
  } = useChat();

  const [input, setInput] = useState('');
  const [activeContext, setActiveContext] = useState<{ ticker: string; title: string } | null>(null);
  const [categories, setCategories] = useState<KalshiCategoryStat[]>([]);
  const [paperBusy, setPaperBusy] = useState<string | null>(null);
  const [paperMsg, setPaperMsg] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    void initSession().then(() => refreshSessions());
  }, [initSession, refreshSessions]);

  useEffect(() => {
    const el = messagesEndRef.current;
    if (el && typeof el.scrollIntoView === 'function') {
      el.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, streamingText, isStreaming]);

  useEffect(() => {
    if (!initialPrompt) return;
    setInput(initialPrompt);
    const ctx = extractTickerFromPrompt(initialPrompt);
    if (ctx) setActiveContext(ctx);
    onPromptConsumed?.();
    // Focus composer so user can send immediately
    queueMicrotask(() => inputRef.current?.focus());
  }, [initialPrompt, onPromptConsumed]);

  useEffect(() => {
    let cancelled = false;
    kalshiApi
      .getCategoryStats()
      .then((stats) => {
        if (!cancelled) setCategories(stats.slice(0, 8));
      })
      .catch(() => {
        if (!cancelled) setCategories([]);
      });
    return () => {
      cancelled = true;
    };
  }, [kalshiContextStatus?.tape_market_count]);

  const livePrompts = useMemo(() => {
    if (kalshiContextStatus?.degraded || categories.length === 0) {
      return FALLBACK_PROMPTS;
    }
    const fromCats = categories.slice(0, 4).map((c) => ({
      label: c.category,
      query: `Analyze the top open Kalshi markets in ${c.category}. Note volume ($${Math.round(c.volume_24h).toLocaleString()} 24h across ${c.count} markets) and flag any that look mispriced.`,
    }));
    return [...fromCats, ...FALLBACK_PROMPTS.slice(0, 2)];
  }, [categories, kalshiContextStatus?.degraded]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || isStreaming) return;
    void sendMessage(input.trim());
    setInput('');
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (!input.trim() || isStreaming) return;
      void sendMessage(input.trim());
      setInput('');
    }
  };

  const handleQuickPrompt = (query: string) => {
    if (isStreaming) return;
    void sendMessage(query);
  };

  const recordPaper = async (message: ChatMessage) => {
    const decision = extractPaperDecision(message.content, {
      ticker: activeContext?.ticker,
      title: activeContext?.title,
    });
    if (!decision) {
      setPaperMsg('Could not parse a YES/NO decision + ticker from this reply. Ask the model for a structured JSON decision.');
      return;
    }
    setPaperBusy(message.id);
    setPaperMsg(null);
    try {
      const id = await kalshiApi.recordPaperDecision(sessionId ?? 'analyst', decision);
      setPaperMsg(
        `Paper ${decision.decision} on ${decision.ticker} recorded (${id.slice(0, 8)}…). Ledger + forecast row written.`,
      );
    } catch (e) {
      setPaperMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setPaperBusy(null);
    }
  };

  const tapeCold = kalshiContextStatus?.degraded === true;
  const tapeCount = kalshiContextStatus?.tape_market_count ?? 0;

  return (
    <section className="page analystPage" aria-label="Analyst workspace">
      <aside className="analystSessions" aria-label="Chat sessions">
        <div className="analystSessionsHeader">
          <span className="panelEyebrow">Sessions</span>
          <button
            type="button"
            className="ghostBtn"
            disabled={isStreaming}
            onClick={() => void initSession()}
          >
            New
          </button>
        </div>
        <ul className="sessionList">
          {sessions.length === 0 && (
            <li className="sessionEmpty muted">No saved threads yet. Send a message to start.</li>
          )}
          {sessions.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                className={`sessionItem ${s.id === sessionId ? 'active' : ''}`}
                disabled={isStreaming}
                onClick={() => void selectSession(s.id)}
              >
                <strong>{s.name || 'Untitled'}</strong>
                <span className="muted">
                  {s.message_count} msg · {new Date(s.updated_at).toLocaleString()}
                </span>
              </button>
              <button
                type="button"
                className="sessionDelete ghostBtn danger"
                title="Delete session"
                disabled={isStreaming}
                onClick={() => {
                  if (window.confirm('Delete this session?')) void deleteSession(s.id);
                }}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <div className="analystMain">
        <header className="kalshiHeader analystHeader">
          <div>
            <p className="panelEyebrow">Analyst</p>
            <h2>Kalshi intelligence</h2>
            <p className="muted">
              Live tape context, structured decisions, and paper journal hooks — not order routing.
            </p>
          </div>
          <div className="analystHeaderMeta">
            <span className={`statusPill ${tapeCold ? 'warn' : 'ok'}`}>
              {tapeCold ? `Tape limited (${tapeCount})` : `Tape ready · ${tapeCount} markets`}
            </span>
            <span className="statusPill muted" title="Configured in Settings → Analyst LLM">
              Model from Settings
            </span>
          </div>
        </header>

        {activeContext && (
          <div className="analystContextChip insightCard accent">
            <div className="analystContextRow">
              <code className="contextTicker">{activeContext.ticker}</code>
              <strong>{activeContext.title}</strong>
              <button type="button" className="ghostBtn" onClick={() => setActiveContext(null)}>
                Dismiss
              </button>
            </div>
            <p className="muted">
              Pinned from Command desk. Replies inject live Kalshi context when the tape is healthy.
            </p>
          </div>
        )}

        {tapeCold && (
          <div className="analystDegraded" role="alert">
            <strong>Limited Kalshi market context</strong>
            {kalshiContextStatus?.reasons?.length ? (
              <ul>
                {kalshiContextStatus.reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            ) : (
              <p className="muted">Market tape is cold or failed to load.</p>
            )}
            {onOpenMarkets && (
              <button type="button" className="primaryBtn" onClick={onOpenMarkets}>
                Open Command desk &amp; refresh tape
              </button>
            )}
          </div>
        )}

        <div className="analystQuickRow">
          {livePrompts.map((p) => (
            <button
              key={p.label}
              type="button"
              className="chipBtn"
              disabled={isStreaming}
              onClick={() => handleQuickPrompt(p.query)}
            >
              {p.label}
            </button>
          ))}
        </div>

        <div className="analystMessages" role="log" aria-live="polite">
          {messages.length === 0 && !isStreaming && (
            <div className="analystEmpty">
              <h3>Ask with the book in view</h3>
              {tapeCold ? (
                <>
                  <p className="muted">
                    The catalog is not loaded, so analysis will be under-informed. Refresh markets first,
                    then return here or use <strong>Analyze with AI</strong> on a contract.
                  </p>
                  {onOpenMarkets && (
                    <button type="button" className="primaryBtn" onClick={onOpenMarkets}>
                      Go to Command desk
                    </button>
                  )}
                </>
              ) : (
                <p className="muted">
                  Tape looks ready ({tapeCount} markets). Use a quick prompt, pin a market from Command
                  desk, or type a question. Prefer structured YES/NO + stake if you want one-click paper.
                </p>
              )}
            </div>
          )}

          {messages.map((msg) => (
            <MessageBubble
              key={msg.id}
              message={msg}
              paperBusy={paperBusy === msg.id}
              onRecordPaper={msg.role === 'assistant' ? () => void recordPaper(msg) : undefined}
            />
          ))}

          {isStreaming && (
            <div className="messageBubble assistantBubble streamingBubble">
              <div className="streamingToolbar">
                <span className="streamingDots" aria-live="polite">
                  {streamingText || streamingThought ? 'Streaming reply…' : 'Analyzing…'}
                </span>
                <button type="button" className="ghostBtn" onClick={cancelStream}>
                  Stop
                </button>
              </div>
              {streamingThought && streamingText && (
                <details className="reasoning" open>
                  <summary>Reasoning stream</summary>
                  <div className="messageContent messageContent--stream">
                    {formatChatText(streamingThought)}
                  </div>
                </details>
              )}
              <div className="messageContent messageContent--stream">
                {streamingText || streamingThought ? (
                  <>
                    {formatChatText(streamingText || streamingThought)}
                    <span className="streamCaret" aria-hidden>
                      ▍
                    </span>
                  </>
                ) : (
                  <span className="muted">Waiting for first tokens…</span>
                )}
              </div>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        {paperMsg && (
          <div className="analystPaperMsg">
            <span>{paperMsg}</span>
            {onOpenPaper && paperMsg.includes('recorded') && (
              <button type="button" className="ghostBtn" onClick={onOpenPaper}>
                Open paper portfolio
              </button>
            )}
          </div>
        )}

        {error && (
          <div className="analystError" role="alert">
            <span>{error}</span>
            <div className="analystErrorActions">
              {lastFailedPrompt && (
                <button type="button" className="primaryBtn" disabled={isStreaming} onClick={() => void retryLast()}>
                  Retry
                </button>
              )}
              <button type="button" className="ghostBtn" onClick={clearError}>
                Dismiss
              </button>
            </div>
          </div>
        )}

        <form className="analystComposer" onSubmit={handleSubmit}>
          <textarea
            ref={inputRef}
            className="analystInput"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={
              activeContext
                ? `Ask about ${activeContext.ticker}… (Enter to send, Shift+Enter for newline)`
                : 'Ask about prediction markets… (Enter to send)'
            }
            disabled={isStreaming}
            rows={2}
          />
          <div className="composerActions">
            {isStreaming ? (
              <button type="button" className="ghostBtn" onClick={cancelStream}>
                Stop
              </button>
            ) : (
              <button type="submit" className="primaryBtn" disabled={!input.trim()}>
                Send
              </button>
            )}
          </div>
        </form>
      </div>
    </section>
  );
}

function MessageBubble({
  message,
  onRecordPaper,
  paperBusy,
}: {
  message: ChatMessage;
  onRecordPaper?: () => void;
  paperBusy?: boolean;
}) {
  const isUser = message.role === 'user';
  // Some models (OpenCode free/thinking) return only reasoning — never hide that.
  const body = (message.content || '').trim() || (message.reasoning || '').trim();
  const hasSeparateReasoning =
    Boolean(message.reasoning?.trim()) && Boolean(message.content?.trim());
  const canPaper = !isUser && onRecordPaper && extractPaperDecision(body) != null;

  return (
    <div className={`messageBubble ${isUser ? 'userBubble' : 'assistantBubble'}`}>
      {hasSeparateReasoning && (
        <details className="reasoning">
          <summary>Reasoning</summary>
          <div className="messageContent">{formatChatText(message.reasoning || '')}</div>
        </details>
      )}
      <div className="messageContent">
        {body ? (
          formatChatText(body)
        ) : (
          <span className="muted">(Empty model response — try another model in Settings.)</span>
        )}
      </div>
      <div className="messageMeta">
        {message.tokens_used != null && <span className="muted">{message.tokens_used} tokens</span>}
        {canPaper && (
          <button type="button" className="ghostBtn" disabled={paperBusy} onClick={onRecordPaper}>
            {paperBusy ? 'Recording…' : 'Record paper decision'}
          </button>
        )}
      </div>
    </div>
  );
}

