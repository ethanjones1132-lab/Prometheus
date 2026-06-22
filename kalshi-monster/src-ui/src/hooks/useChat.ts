import { useState, useCallback, useRef } from 'react';
import { chatApi } from '../services/tauri';
import type { ChatMessage } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sessionIdRef = useRef<string | null>(null);

  const initSession = useCallback(async () => {
    const session = await chatApi.newSession();
    sessionIdRef.current = session.id;
    setMessages([]);
    setError(null);
    return session.id;
  }, []);

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
        if (stream) {
          await chatApi.sendMessageStream(content, sessionId);
          // Streaming path emits chunks via Tauri events; fall back to polling history.
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
    [initSession],
  );

  return {
    messages,
    isStreaming,
    error,
    sendMessage,
    initSession,
    sessionId: sessionIdRef.current,
  };
}