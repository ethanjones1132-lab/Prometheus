import './index.css';
import { useState } from 'react';
import { ChatView } from './components/ChatView';
import { KalshiPredictionsPanel } from './components/KalshiPredictionsPanel';
import { KalshiView } from './components/KalshiView';
import { PropsView } from './components/PropsView';
import { SettingsView } from './components/SettingsView';

type Tab = 'props' | 'markets' | 'chat' | 'predictions' | 'settings';

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>('props');

  return (
    <div className="appShell">
      <aside className="sidebar">
        <div className="brand">
          <div className="logo">KM</div>
          <div>
            <strong>Kalshi Monster</strong>
            <span>Prediction market intelligence</span>
          </div>
        </div>

        {[
          { id: 'props', label: '🎯 Prop board' },
          { id: 'markets', label: '📊 Kalshi dashboard' },
          { id: 'chat', label: '🧠 Analyst chat' },
          { id: 'predictions', label: '📈 Prediction log' },
          { id: 'settings', label: '⚙️ Settings' },
        ].map((tab) => (
          <button
            key={tab.id}
            className={`navButton ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveTab(tab.id as Tab)}
          >
            {tab.label}
          </button>
        ))}
      </aside>

      <main className="main">
        {activeTab === 'props' && <PropsView />}
        {activeTab === 'markets' && <KalshiView />}
        {activeTab === 'chat' && <ChatView />}
        {activeTab === 'predictions' && (
          <section className="page kalshiPage">
            <header className="kalshiHeader">
              <div>
                <h2>Prediction log</h2>
                <p className="muted">Kalshi paper trades with contract-side grading and PnL tracking.</p>
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
