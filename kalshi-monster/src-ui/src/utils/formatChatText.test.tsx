import { render, screen } from '@testing-library/react';
import { describe, expect, test } from 'vitest';
import { formatChatText } from './formatChatText';

describe('formatChatText', () => {
  test('preserves newlines and bold', () => {
    const { container } = render(<>{formatChatText('Line one\n**bold bit**\nLine three')}</>);
    expect(container.textContent).toContain('Line one');
    expect(container.textContent).toContain('bold bit');
    expect(screen.getByText('bold bit').tagName).toBe('STRONG');
  });

  test('renders fenced code blocks', () => {
    const text = 'Before\n```json\n{"a":1}\n```\nAfter';
    const { container } = render(<>{formatChatText(text)}</>);
    expect(container.querySelector('pre.chatCodeBlock')).toBeTruthy();
    expect(container.textContent).toContain('"a":1');
  });
});
