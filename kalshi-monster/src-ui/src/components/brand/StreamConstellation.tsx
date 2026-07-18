/**
 * Three orbiting embers shown while the analyst is thinking/streaming —
 * a miniature constellation in place of a spinner.
 */
export function StreamConstellation() {
  return (
    <span className="streamConstellation" role="status" aria-label="Analyst is composing">
      <i />
      <i />
      <i />
    </span>
  );
}
