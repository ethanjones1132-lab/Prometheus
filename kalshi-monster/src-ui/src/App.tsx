import './index.css';
import { useState } from 'react';
import { CalibrationView } from './components/CalibrationView';
import { ChatView } from './components/ChatView';
import { KalshiPredictionsPanel } from './components/KalshiPredictionsPanel';
import { KalshiView } from './components/KalshiView';
import { SettingsView } from './components/SettingsView';
import { WorldMarketsView } from './components/WorldMarketsView';
import { ConstellationBackdrop } from './components/brand/ConstellationBackdrop';
import { PrometheusMark } from './components/brand/PrometheusMark';
import { LiveDot } from './components/brand/LiveDot';
import {
  AnalystEyeIcon,
  CalibrationIcon,
  CommandDeskIcon,
  PortfolioIcon,
  SettingsIcon,
  WorldMarketsIcon,
} from './components/brand/icons';

type Tab = 'markets' | 'world' | 'chat' | 'predictions' | 'calibration' | 'settings';

const tabs: Array<{ id: Tab; label: string; icon: () => JSX.Element }> = [
  { id: 'markets', label: 'Command desk', icon: CommandDeskIcon },
  { id: 'world', label: 'World markets', icon: WorldMarketsIcon },
  { id: 'chat', label: 'Analyst', icon: AnalystEyeIcon },
  { id: 'predictions', label: 'Paper portfolio', icon: PortfolioIcon },
  { id: 'calibration', label: 'Calibration', icon: CalibrationIcon },
  { id: 'settings', label: 'Settings', icon: SettingsIcon },
];

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('markets');
  const [analystPrompt, setAnalystPrompt] = useState<string | null>(null);

  const openAnalyst = (prompt: string) => {
    setAnalystPrompt(prompt);
    setActiveTab('chat');
  };

  return (
    <div className="appShell">
      <ConstellationBackdrop />
      <aside className="sidebar">
        <div className="brand">
          <div className="brandMark">
            <PrometheusMark variant="emblem" />
          </div>
          <div className="brandWord">
            <strong className="wordmark">PROMETHEUS</strong>
            <span className="brandSub">Prediction market intelligence</span>
          </div>
        </div>

        <div className="sidebarIntel">
          <span>Default mode</span>
          <strong>Kalshi-first</strong>
          <p>Markets, analyst prompts, and paper risk stay centered on event contracts.</p>
        </div>

        <nav className="nav" aria-label="Primary">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={`navButton ${activeTab === tab.id ? 'active' : ''}`}
                onClick={() => setActiveTab(tab.id)}
              >
                <span className="navIcon">
                  <Icon />
                </span>
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="sidebarFoot">
          <span className="liveLabel">
            <LiveDot />
            Edge engine
          </span>
          <span>v0.8.0</span>
        </div>
      </aside>

      <main className="main">
        {activeTab === 'markets' && <KalshiView onAnalyzeMarket={openAnalyst} />}
        {activeTab === 'world' && <WorldMarketsView />}
        {activeTab === 'chat' && (
          <ChatView
            initialPrompt={analystPrompt}
            onPromptConsumed={() => setAnalystPrompt(null)}
            onOpenMarkets={() => setActiveTab('markets')}
            onOpenPaper={() => setActiveTab('predictions')}
          />
        )}
        {activeTab === 'predictions' && (
          <section className="page kalshiPage">
            <header className="kalshiHeader">
              <div>
                <p className="eyebrow">Paper ledger</p>
                <h2>Paper trades</h2>
                <p className="muted">Kalshi paper decisions with contract-side grading and PnL tracking.</p>
              </div>
            </header>
            <KalshiPredictionsPanel />
          </section>
        )}
        {activeTab === 'calibration' && <CalibrationView />}
        {activeTab === 'settings' && <SettingsView />}
      </main>
    </div>
  );
}
