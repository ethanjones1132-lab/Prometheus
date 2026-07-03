import './index.css';
import { useState } from 'react';
import { ChatView } from './components/ChatView';
import { KalshiPredictionsPanel } from './components/KalshiPredictionsPanel';
import { KalshiView } from './components/KalshiView';
import { SettingsView } from './components/SettingsView';

type Tab = 'markets' | 'chat' | 'predictions' | 'settings';

const tabs: Array<{ id: Tab; label: string }> = [
  { id: 'markets', label: 'Command desk' },
  { id: 'chat', label: 'Analyst' },
  { id: 'predictions', label: 'Paper portfolio' },
  { id: 'settings', label: 'Settings' },
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
      <aside className="sidebar">
        <div className="brand">
          <div className="logo">KM</div>
          <div>
            <strong>Kalshi Monster</strong>
            <span>Event market command desk</span>
          </div>
        </div>

        <div className="sidebarIntel">
          <span>Default mode</span>
          <strong>Kalshi-first</strong>
          <p>Markets, analyst prompts, and paper risk stay centered on event contracts.</p>
        </div>

        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`navButton ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </aside>

      <main className="main">
        {activeTab === 'markets' && <KalshiView onAnalyzeMarket={openAnalyst} />}
        {activeTab === 'chat' && (
          <ChatView
            initialPrompt={analystPrompt}
            onPromptConsumed={() => setAnalystPrompt(null)}
          />
        )}
        {activeTab === 'predictions' && (
          <section className="page kalshiPage">
            <header className="kalshiHeader">
              <div>
                <h2>Paper trades</h2>
                <p className="muted">Kalshi paper decisions with contract-side grading and PnL tracking.</p>
              </div>
            </header>
            <KalshiPredictionsPanel />
          </section>
        )}
        {activeTab === 'settings' && <SettingsView />}
      </main>
    </div>
  );
}
