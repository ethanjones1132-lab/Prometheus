import { useState, useCallback, useRef } from 'react';
import { chatApi } from '../services/tauri';
import type { ChatMessage } from '../types';

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sessionIdRef = useRef<string | null>(null);

  const initSession = useCallback(async () => {
    const id = await chatApi.newSession();
    sessionIdRef.current = id;
    setMessages([]);
    setError(null);
    return id;
  }, []);

  const sendMessage = useCallback(async (content: string, stream = true) => {
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
    setMessages(prev => [...prev, userMsg]);
    setIsStreaming(true);
    setError(null);

    try {
      const response = await chatApi.send({
        session_id: sessionId,
        message: content,
        stream,
      });
      setMessages(prev => [...prev, response.message]);
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e);
      setError(errMsg);
    } finally {
      setIsStreaming(false);
    }
  }, [initSession]);

  return {
    messages,
    isStreaming,
    error,
    sendMessage,
    initSession,
    sessionId: sessionIdRef.current,
  };
}
