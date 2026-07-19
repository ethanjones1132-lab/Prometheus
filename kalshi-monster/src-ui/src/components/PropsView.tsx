import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { analysisApi } from '../services/tauri';
import type { EdgeAnalysisInput, PropScore, PropScoreInput, ScoredProp } from '../types';

type SortKey = 'edge_score' | 'composite_score' | 'player_name';

const DEMO_PROPS: EdgeAnalysisInput[] = [
  {
    player_name: 'Patrick Mahomes',
    stat_category: 'Passing Yards',
    line: 285.5,
    pick_type: 'Over',
    projection: 298.0,
    season_avg: 272.0,
    last3_avg: 301.0,
    is_home: true,
    defense_rank: 18,
    consistency_score: 0.82,
  },
  {
    player_name: 'Tyreek Hill',
    stat_category: 'Receiving Yards',
    line: 78.5,
    pick_type: 'Over',
    projection: 86.0,
    season_avg: 81.0,
    last3_avg: 92.0,
    is_home: false,
    defense_rank: 24,
    consistency_score: 0.74,
  },
  {
    player_name: 'Josh Allen',
    stat_category: 'Passing TDs',
    line: 1.5,
    pick_type: 'Over',
    projection: 2.1,
    season_avg: 1.9,
    last3_avg: 2.3,
    is_home: true,
    defense_rank: 12,
    consistency_score: 0.88,
  },
  {
    player_name: 'CeeDee Lamb',
    stat_category: 'Receptions',
    line: 6.5,
    pick_type: 'Over',
    projection: 7.8,
    season_avg: 7.1,
    last3_avg: 8.2,
    is_home: true,
    defense_rank: 20,
    usage_rate: 0.29,
    consistency_score: 0.79,
  },
];

function toPropScore(result: { scored: ScoredProp }): PropScore {
  const { scored } = result;
  return {
    edge_pct: scored.edge_score,
    expected_value_pct: scored.expected_value,
    risk: scored.risks[0] ?? scored.tier,
    recommendation: scored.recommendation,
    reasoning: scored.key_factors.join(' · ') || `${scored.tier} tier — ${scored.confidence} confidence`,
  };
}

function scoreInputToAnalysis(input: PropScoreInput): EdgeAnalysisInput {
  const pickType = input.projection >= input.line ? 'Over' : 'Under';
  return {
    player_name: 'Manual entry',
    stat_category: 'Custom',
    line: input.line,
    pick_type: pickType,
    projection: input.projection,
    season_avg: input.projection,
    last3_avg: input.projection,
    is_home: true,
    consistency_score: input.confidence / 100,
  };
}

function stagger(i: number): CSSProperties {
  return {
    '--i': i,
    animation: 'fadeRise 0.55s var(--ease-luxe) both',
    animationDelay: 'calc(var(--i, 0) * 70ms)',
  } as CSSProperties;
}

export function PropsView() {
  const [props, setProps] = useState<ScoredProp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('edge_score');
  const [scoreInput, setScoreInput] = useState<PropScoreInput>({
    line: 25.5,
    projection: 27.0,
    implied_probability: 0.5,
    model_probability: 0.58,
    confidence: 70,
  });
  const [score, setScore] = useState<PropScore | null>(null);

  const loadProps = async () => {
    setLoading(true);
    setError(null);
    try {
      const ctx = await analysisApi.analyzeMultiple(DEMO_PROPS);
      setProps(ctx.scored_props);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadProps();
  }, []);

  const filteredProps = useMemo(() => {
    const q = query.trim().toLowerCase();
    const copy = q
      ? props.filter(
          (p) =>
            p.player_name.toLowerCase().includes(q) ||
            p.stat_category.toLowerCase().includes(q) ||
            p.pick_type.toLowerCase().includes(q),
        )
      : [...props];
    copy.sort((a, b) => {
      if (sortKey === 'player_name') {
        return a.player_name.localeCompare(b.player_name);
      }
      return (b[sortKey] as number) - (a[sortKey] as number);
    });
    return copy;
  }, [props, query, sortKey]);

  const handleScore = async () => {
    setError(null);
    setScore(null);
    try {
      const result = await analysisApi.analyzeProp(scoreInputToAnalysis(scoreInput));
      setScore(toPropScore(result));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="page">
      <section className="hero">
        <div>
          <p className="eyebrow">Edge analyzer</p>
          <h1>Sports prop analysis desk</h1>
          <p>
            Demo prop board powered by the Rust analysis engine (<code>analyze_multiple_props</code>).
            Live PrizePicks feed is not wired in this recovery build — use Kalshi dashboard for market data.
          </p>
        </div>
        <div className="heroMetric">
          <span>{filteredProps.length}</span>
          <small>scored props</small>
        </div>
      </section>

      <section className="grid two">
        <div className="card" style={{ '--i': 0 } as CSSProperties}>
          <div className="cardHeader">
            <div>
              <p className="eyebrow" style={{ marginBottom: 4 }}>Scored board</p>
              <h2>Prop board</h2>
            </div>
            <button type="button" className="ghostBtn" onClick={() => void loadProps()}>
              Refresh
            </button>
          </div>
          <div className="toolbar">
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search player, stat, pick…"
            />
            <select value={sortKey} onChange={(event) => setSortKey(event.target.value as SortKey)}>
              <option value="edge_score">Highest edge</option>
              <option value="composite_score">Highest composite</option>
              <option value="player_name">Player A-Z</option>
            </select>
          </div>

          {loading && <div className="state">Running analysis pipeline…</div>}
          {error && <div className="state error">{error}</div>}
          {!loading && !error && filteredProps.length === 0 && (
            <div className="state">No props match your filter.</div>
          )}

          <div className="propList">
            {filteredProps.map((prop, idx) => (
              <article
                className="propCard"
                key={`${prop.player_name}-${prop.stat_category}`}
                style={stagger(idx)}
              >
                <div className="propTop">
                  <span className="sport nfl">{prop.tier}</span>
                  <span className="risk">{prop.confidence}</span>
                </div>
                <h3>{prop.player_name}</h3>
                <p>
                  {prop.pick_type} {prop.line} · {prop.stat_category}
                </p>
                <div className="propGrid">
                  <div>
                    <span>Edge</span>
                    <strong className={prop.edge_score >= 0 ? 'positive' : 'negative'}>
                      {prop.edge_score.toFixed(1)}%
                    </strong>
                  </div>
                  <div>
                    <span>Composite</span>
                    <strong>{prop.composite_score.toFixed(0)}</strong>
                  </div>
                  <div>
                    <span>Win prob</span>
                    <strong>{prop.win_probability.toFixed(0)}%</strong>
                  </div>
                  <div>
                    <span>Kelly</span>
                    <strong>{prop.kelly_stake_pct.toFixed(2)}%</strong>
                  </div>
                </div>
                <div className="propFooter">
                  <span>{prop.recommendation}</span>
                </div>
                <p className="reasoning">{prop.key_factors.join(' · ')}</p>
              </article>
            ))}
          </div>
        </div>

        <div className="stack">
          <div className="card" style={{ '--i': 1 } as CSSProperties}>
            <p className="eyebrow" style={{ marginBottom: 4 }}>Manual entry</p>
            <h2>Quick prop scorer</h2>
            <p className="muted">Runs <code>analyze_prop</code> on a manual line/projection pair.</p>
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
                  onChange={(event) =>
                    setScoreInput({ ...scoreInput, projection: Number(event.target.value) })
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
                  onChange={(event) =>
                    setScoreInput({ ...scoreInput, confidence: Number(event.target.value) })
                  }
                />
              </label>
            </div>
            <button type="button" className="primaryBtn" onClick={() => void handleScore()}>
              Score prop
            </button>
            {score && (
              <div className="scoreBox">
                <strong>{score.recommendation}</strong>
                <span>{score.edge_pct.toFixed(1)}% edge · EV {score.expected_value_pct.toFixed(1)}%</span>
                <p>{score.reasoning}</p>
              </div>
            )}
          </div>

          <div className="card" style={{ '--i': 2 } as CSSProperties}>
            <p className="eyebrow" style={{ marginBottom: 4 }}>Diagnostics</p>
            <h2>IPC status</h2>
            <ul className="checkList">
              <li>Chat → <code>send_message</code> / <code>new_chat_session</code></li>
              <li>Settings → <code>get_config</code> / <code>save_config</code></li>
              <li>Props → <code>analyze_prop</code> / <code>analyze_multiple_props</code></li>
              <li>Kalshi surface uses dedicated <code>kalshi.ts</code> service</li>
            </ul>
          </div>
        </div>
      </section>
    </div>
  );
}