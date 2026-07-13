import { useState, useCallback, useRef, useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { chatApi } from '../services/tauri';
import { kalshiApi, type KalshiChatContextStatus } from '../services/kalshi';
import type { ChatMessage, ChatSession } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamingText, setStreamingText] = useState('');
  const [streamingThought, setStreamingThought] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [lastFailedPrompt, setLastFailedPrompt] = useState<string | null>(null);
  const [kalshiContextStatus, setKalshiContextStatus] = useState<KalshiChatContextStatus | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const streamUnsubsRef = useRef<UnlistenFn[]>([]);
  const streamAbortRef = useRef(false);
  const streamingTextRef = useRef('');
  const streamingThoughtRef = useRef('');

  const refreshKalshiContextStatus = useCallback(async () => {
    try {
      const status = await kalshiApi.getChatContextStatus();
      setKalshiContextStatus(status);
    } catch {
      // Non-fatal
    }
  }, []);

  const refreshSessions = useCallback(async () => {
    try {
      const list = await chatApi.listSessions();
      // newest first
      list.sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1));
      setSessions(list);
    } catch {
      // sessions optional until disk path works
    }
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    void listen<{ session_id: string; status: KalshiChatContextStatus }>(
      'chat-kalshi-context',
      (event) => {
        if (event.payload.session_id === sessionIdRef.current) {
          setKalshiContextStatus(event.payload.status);
        }
      },
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const clearStreamListeners = useCallback(async () => {
    const subs = streamUnsubsRef.current;
    streamUnsubsRef.current = [];
    await Promise.all(subs.map((u) => Promise.resolve(u())));
  }, []);

  const initSession = useCallback(async () => {
    const session = await chatApi.newSession();
    sessionIdRef.current = session.id;
    setSessionId(session.id);
    setMessages([]);
    setError(null);
    setLastFailedPrompt(null);
    setStreamingText('');
    setStreamingThought('');
    await refreshKalshiContextStatus();
    await refreshSessions();
    return session.id;
  }, [refreshKalshiContextStatus, refreshSessions]);

  const selectSession = useCallback(
    async (id: string) => {
      if (isStreaming) return;
      await clearStreamListeners();
      sessionIdRef.current = id;
      setSessionId(id);
      setError(null);
      setStreamingText('');
      setStreamingThought('');
      try {
        const history = await chatApi.getHistory(id);
        setMessages(history);
      } catch (e) {
        setMessages([]);
        setError(e instanceof Error ? e.message : String(e));
      }
      await refreshKalshiContextStatus();
    },
    [clearStreamListeners, isStreaming, refreshKalshiContextStatus],
  );

  const deleteSession = useCallback(
    async (id: string) => {
      await chatApi.deleteSession(id);
      if (sessionIdRef.current === id) {
        await initSession();
      } else {
        await refreshSessions();
      }
    },
    [initSession, refreshSessions],
  );

  const renameSession = useCallback(
    async (id: string, newName: string) => {
      await chatApi.renameSession(id, newName);
      await refreshSessions();
    },
    [refreshSessions],
  );

  const sendMessage = useCallback(
    async (content: string) => {
      if (!sessionIdRef.current) {
        await initSession();
      }
      const sid = sessionIdRef.current!;
      streamAbortRef.current = false;

      const userMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: 'user',
        content,
        timestamp: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, userMsg]);
      setIsStreaming(true);
      streamingTextRef.current = '';
      streamingThoughtRef.current = '';
      setStreamingText('');
      setStreamingThought('');
      setError(null);
      setLastFailedPrompt(null);

      await clearStreamListeners();

      const onChunk = (chunk: string) => {
        if (streamAbortRef.current) return;
        streamingTextRef.current += chunk;
        setStreamingText(streamingTextRef.current);
      };
      const onThought = (thought: string) => {
        if (streamAbortRef.current) return;
        streamingThoughtRef.current = streamingThoughtRef.current
          ? `${streamingThoughtRef.current}\n${thought}`
          : thought;
        setStreamingThought(streamingThoughtRef.current);
      };

      try {
        // Do not await tape status before the LLM call — that added first-token latency.
        void refreshKalshiContextStatus();

        const unsubs = await Promise.all([
          listen<{ session_id: string; chunk: string }>('stream-chunk', (ev) => {
            if (ev.payload.session_id === sid) onChunk(ev.payload.chunk);
          }),
          listen<{ session_id: string; thought: string }>('stream-thought', (ev) => {
            if (ev.payload.session_id === sid) onThought(ev.payload.thought);
          }),
          listen<string>('stream-done', async (ev) => {
            if (ev.payload !== sid) return;
          }),
          listen<{ session_id: string; error: string }>('stream-error', (ev) => {
            if (ev.payload.session_id === sid) {
              setError(ev.payload.error);
              setLastFailedPrompt(content);
            }
          }),
        ]);
        streamUnsubsRef.current = unsubs;

        await chatApi.sendMessageStream(content, sid);

        if (!streamAbortRef.current) {
          const history = await chatApi.getHistory(sid);
          // If history has an empty assistant tail but we streamed text/thoughts,
          // keep the streamed body so the UI never flashes blank.
          const streamedBody = streamingTextRef.current || streamingThoughtRef.current;
          if (history.length > 0) {
            const last = history[history.length - 1];
            const emptyAssistant =
              last?.role === 'assistant' &&
              !(last.content || '').trim() &&
              !(last.reasoning || '').trim();
            if (emptyAssistant && streamedBody) {
              history[history.length - 1] = { ...last, content: streamedBody };
            }
          }
          setMessages(history);
          streamingTextRef.current = '';
          streamingThoughtRef.current = '';
          setStreamingText('');
          setStreamingThought('');
        }
        await refreshSessions();
      } catch (e) {
        const errMsg = e instanceof Error ? e.message : String(e);
        setError(errMsg);
        setLastFailedPrompt(content);
      } finally {
        await clearStreamListeners();
        setIsStreaming(false);
      }
    },
    [initSession, refreshKalshiContextStatus, refreshSessions, clearStreamListeners],
  );

  const cancelStream = useCallback(() => {
    streamAbortRef.current = true;
    setIsStreaming(false);
    setStreamingText((text) => {
      if (text.trim()) {
        setMessages((prev) => [
          ...prev,
          {
            id: crypto.randomUUID(),
            role: 'assistant',
            content: `${text}\n\n_(stream stopped in UI — backend may still finish)_`,
            timestamp: new Date().toISOString(),
          },
        ]);
      }
      return '';
    });
    setStreamingThought('');
    void clearStreamListeners();
  }, [clearStreamListeners]);

  const retryLast = useCallback(async () => {
    if (!lastFailedPrompt || isStreaming) return;
    const prompt = lastFailedPrompt;
    setLastFailedPrompt(null);
    await sendMessage(prompt);
  }, [lastFailedPrompt, isStreaming, sendMessage]);

  const clearError = useCallback(() => setError(null), []);

  return {
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
    renameSession,
    refreshSessions,
    cancelStream,
    retryLast,
    clearError,
    kalshiContextStatus,
    refreshKalshiContextStatus,
  };
}
