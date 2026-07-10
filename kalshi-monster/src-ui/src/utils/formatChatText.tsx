import type { ReactNode } from 'react';

/**
 * Lightweight chat formatting: preserves newlines, code fences, bold, and
 * simple list/heading lines. No external markdown dependency.
 */
export function formatChatText(text: string): ReactNode {
  if (!text) return null;

  const parts: ReactNode[] = [];
  const fenceRe = /```([a-zA-Z0-9_+-]*)\n?([\s\S]*?)```/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = fenceRe.exec(text)) !== null) {
    if (match.index > last) {
      parts.push(
        <span key={`t-${key++}`} className="chatTextBlock">
          {formatInlineBlocks(text.slice(last, match.index), key)}
        </span>,
      );
      key += 50;
    }
    const lang = match[1] || 'text';
    parts.push(
      <pre key={`c-${key++}`} className="chatCodeBlock" data-lang={lang}>
        <code>{match[2].replace(/\n$/, '')}</code>
      </pre>,
    );
    last = match.index + match[0].length;
  }

  if (last < text.length) {
    parts.push(
      <span key={`t-${key++}`} className="chatTextBlock">
        {formatInlineBlocks(text.slice(last), key)}
      </span>,
    );
  }

  return <>{parts}</>;
}

function formatInlineBlocks(text: string, keyBase: number): ReactNode {
  const lines = text.split('\n');
  return lines.map((line, i) => {
    const k = `${keyBase}-L${i}`;
    const suffix = i < lines.length - 1 ? '\n' : '';
    if (/^#{1,3}\s+/.test(line)) {
      const level = (line.match(/^#+/) || ['#'])[0].length;
      const body = line.replace(/^#{1,3}\s+/, '');
      return (
        <span key={k} className={`chatHeading h${level}`}>
          {formatInline(body)}
          {suffix}
        </span>
      );
    }
    if (/^[-*•]\s+/.test(line)) {
      return (
        <span key={k} className="chatListItem">
          • {formatInline(line.replace(/^[-*•]\s+/, ''))}
          {suffix}
        </span>
      );
    }
    if (/^\d+\.\s+/.test(line)) {
      return (
        <span key={k} className="chatListItem">
          {formatInline(line)}
          {suffix}
        </span>
      );
    }
    return (
      <span key={k}>
        {formatInline(line)}
        {suffix}
      </span>
    );
  });
}

function formatInline(text: string): ReactNode {
  const nodes: ReactNode[] = [];
  const re = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) nodes.push(text.slice(last, m.index));
    if (m[1] != null) {
      nodes.push(
        <strong key={`b${i++}`} className="chatBold">
          {m[1]}
        </strong>,
      );
    } else if (m[2] != null) {
      nodes.push(
        <code key={`i${i++}`} className="chatInlineCode">
          {m[2]}
        </code>,
      );
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes.length === 1 && typeof nodes[0] === 'string' ? nodes[0] : <>{nodes}</>;
}
