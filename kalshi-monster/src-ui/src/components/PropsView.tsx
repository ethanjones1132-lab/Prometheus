import { useEffect, useMemo, useState } from 'react';
import { propsApi } from '../services/tauri';
import type { PropPick, PropScore, PropScoreInput } from '../types';

type SortKey = 'edge_pct' | 'confidence' | 'projection_gap' | 'player';

export function PropsView() {
  const [props, setProps] = useState<PropPick[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('edge_pct');
  const [scoreInput, setScoreInput] = useState<PropScoreInput>({
    line: 25.5,
    projection: 27.0,
    implied_probability: 0.5,
    model_probability: 0.58,
    confidence: 70,
  });
  const [score, setScore] = useState<PropScore | null>(null);

  const loadProps = async (filter?: string) => {
    setLoading(true);
    setError(null);
    try {
      const data = await propsApi.list(filter || undefined, 50);
      setProps(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadProps();
  }, []);

  const sortedProps = useMemo(() => {
    const copy = [...props];
    copy.sort((a, b) => {
      if (sortKey === 'projection_gap') {
        return Math.abs(b.projection - b.line) - Math.abs(a.projection - a.line);
      }
      if (sortKey === 'player') {
        return a.player.localeCompare(b.player);
      }
      return (b[sortKey] as number) - (a[sortKey] as number);
    });
    return copy;
  }, [props, sortKey]);

  const handleScore = async () => {
    setError(null);
    setScore(null);
    try {
      const next = await propsApi.score(scoreInput);
      setScore(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="page">
      <section className="hero">
        <div>
          <p className="eyebrow">PrizePicks Monster</p>
          <h1>Sports prop prediction desk</h1>
          <p>
            Enterprise-grade prop triage for PrizePicks-style player props. Kalshi-compatible market tools remain
            available, but this workspace is optimized around sports props, edges, confidence, and bankroll discipline.
          </p>
        </div>
        <div className="heroMetric">
          <span>{props.length}</span>
          <small>seeded props</small>
        </div>
      </section>

      <section className="grid two">
        <div className="card">
          <div className="cardHeader">
            <h2>Prop board</h2>
            <button className="button secondary" onClick={() => loadProps(query)}>
              Refresh
            </button>
          </div>
          <div className="toolbar">
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search player, team, prop, league..."
              onKeyDown={(event) => {
                if (event.key === 'Enter') loadProps(query);
              }}
            />
            <select value={sortKey} onChange={(event) => setSortKey(event.target.value as SortKey)}>
              <option value="edge_pct">Highest edge</option>
              <option value="confidence">Highest confidence</option>
              <option value="projection_gap">Largest projection gap</option>
              <option value="player">Player A-Z</option>
            </select>
          </div>

          {loading && <div className="state">Loading sports prop board...</div>}
          {error && <div className="state error">{error}</div>}
          {!loading && !error && sortedProps.length === 0 && (
            <div className="state">No props found. Try a different player, sport, or prop type.</div>
          )}

          <div className="propList">
            {sortedProps.map((prop) => (
              <article className="propCard" key={prop.id}>
                <div className="propTop">
                  <span className={`sport ${prop.sport}`}>{prop.league}</span>
                  <span className="risk">{prop.risk} risk</span>
                </div>
                <h3>{prop.player}</h3>
                <p>{prop.game}</p>
                <div className="propGrid">
                  <div>
                    <span>Prop</span>
                    <strong>{prop.prop_type}</strong>
                  </div>
                  <div>
                    <span>Line</span>
                    <strong>{prop.line}</strong>
                  </div>
                  <div>
                    <span>Projection</span>
                    <strong>{prop.projection}</strong>
                  </div>
                  <div>
                    <span>Edge</span>
                    <strong className={prop.edge_pct >= 0 ? 'positive' : 'negative'}>{prop.edge_pct.toFixed(1)}%</strong>
                  </div>
                </div>
                <div className="meter">
                  <div style={{ width: `${Math.min(100, prop.confidence)}%` }} />
                </div>
                <div className="propFooter">
                  <span>{prop.recommendation}</span>
                  <small>{new Date(prop.updated_at).toLocaleString()}</small>
                </div>
                <p className="reasoning">{prop.reasoning}</p>
              </article>
            ))}
          </div>
        </div>

        <div className="stack">
          <div className="card">
            <h2>Quick prop scorer</h2>
            <p className="muted">
              Normalize a manual line/projection pair into a recommendation without exposing any API secrets.
            </p>
            <div className="formGrid">
              <label>
                Line
                <input
                  type="number"
                  step="0.1"
                  value={scoreInput.line}
                  onChange={(event) => setScoreInput({ ...scoreInput, line: Number(event.target.value) })}
                />
              </label>
              <label>
                Projection
                <input
                  type="number"
                  step="0.1"
                  value={scoreInput.projection}
                  onChange={(event) => setScoreInput({ ...scoreInput, projection: Number(event.target.value) })}
                />
              </label>
              <label>
                Implied probability
                <input
                  type="number"
                  min="0"
                  max="1"
                  step="0.01"
                  value={scoreInput.implied_probability}
                  onChange={(event) =>
                    setScoreInput({ ...scoreInput, implied_probability: Number(event.target.value) })
                  }
                />
              </label>
              <label>
                Model probability
                <input
                  type="number"
                  min="0"
                  max="1"
                  step="0.01"
                  value={scoreInput.model_probability}
                  onChange={(event) =>
                    setScoreInput({ ...scoreInput, model_probability: Number(event.target.value) })
                  }
                />
              </label>
              <label>
                Confidence %
                <input
                  type="number"
                  min="0"
                  max="100"
                  value={scoreInput.confidence}
                  onChange={(event) => setScoreInput({ ...scoreInput, confidence: Number(event.target.value) })}
                />
              </label>
            </div>
            <button className="button primary" onClick={handleScore}>
              Score prop
            </button>
            {score && (
              <div className="scoreBox">
                <strong>{score.recommendation}</strong>
                <span>{score.edge_pct.toFixed(1)}% edge</span>
                <p>{score.reasoning}</p>
              </div>
            )}
          </div>

          <div className="card">
            <h2>Runtime quality gates</h2>
            <ul className="checkList">
              <li>Prop board backed by Tauri command, not static HTML.</li>
              <li>Config endpoint redacts API keys and webhook secrets.</li>
              <li>Kelly calculator accepts decimal and cent probabilities.</li>
              <li>Prediction writes validate required fields and finite stake values.</li>
              <li>Kalshi files remain compatibility scaffolding, not the product face.</li>
            </ul>
          </div>
        </div>
      </section>
    </div>
  );
}
