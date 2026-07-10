import { useState, useCallback, useRef, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { chatApi } from '../services/tauri';
import { kalshiApi, type KalshiChatContextStatus } from '../services/kalshi';
import type { ChatMessage } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [kalshiContextStatus, setKalshiContextStatus] = useState<KalshiChatContextStatus | null>(null);
  const sessionIdRef = useRef<string | null>(null);

  const refreshKalshiContextStatus = useCallback(async () => {
    try {
      const status = await kalshiApi.getChatContextStatus();
      setKalshiContextStatus(status);
    } catch {
      // Non-fatal — banner may stay stale until next send or IPC event
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
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

  const initSession = useCallback(async () => {
    const session = await chatApi.newSession();
    sessionIdRef.current = session.id;
    setMessages([]);
    setError(null);
    await refreshKalshiContextStatus();
    return session.id;
  }, [refreshKalshiContextStatus]);

  const sendMessage = useCallback(
    async (content: string, stream = false) => {
      if (!sessionIdRef.current) {
        await initSession();
      }
      const sessionId = sessionIdRef.current!;

      const userMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: 'user',
        content,
        timestamp: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, userMsg]);
      setIsStreaming(true);
      setError(null);

      try {
        await refreshKalshiContextStatus();
        if (stream) {
          await chatApi.sendMessageStream(content, sessionId);
          const history = await chatApi.getHistory(sessionId);
          setMessages(history);
        } else {
          const response = await chatApi.sendMessage(content, sessionId);
          const assistantMsg: ChatMessage = {
            id: crypto.randomUUID(),
            role: 'assistant',
            content: response.content,
            reasoning: response.reasoning,
            timestamp: new Date().toISOString(),
            tokens_used: response.tokens_used,
          };
          setMessages((prev) => [...prev, assistantMsg]);
        }
      } catch (e) {
        const errMsg = e instanceof Error ? e.message : String(e);
        setError(errMsg);
      } finally {
        setIsStreaming(false);
      }
    },
    [initSession, refreshKalshiContextStatus],
  );

  return {
    messages,
    isStreaming,
    error,
    sendMessage,
    initSession,
    sessionId: sessionIdRef.current,
    kalshiContextStatus,
  };
}