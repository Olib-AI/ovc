import { useParams, useSearchParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { Zap } from 'lucide-react';
import ActionsDashboard from '../components/ActionsDashboard.tsx';
import ActionsHistory from '../components/ActionsHistory.tsx';
import ActionsSettings from '../components/ActionsSettings.tsx';
import ActionConfigEditor from '../components/ActionConfigEditor.tsx';
import ActionSecretsPanel from '../components/ActionSecretsPanel.tsx';

type ActionsTab = 'dashboard' | 'history' | 'config' | 'secrets' | 'settings';

const TABS: { key: ActionsTab; label: string }[] = [
  { key: 'dashboard', label: 'Dashboard' },
  { key: 'history', label: 'History' },
  { key: 'config', label: 'Config' },
  { key: 'secrets', label: 'Secrets' },
  { key: 'settings', label: 'Settings' },
];

function isActionsTab(value: string | null): value is ActionsTab {
  return (
    value === 'dashboard' ||
    value === 'history' ||
    value === 'config' ||
    value === 'secrets' ||
    value === 'settings'
  );
}

function ActionsPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Actions \u2014 OVC`);
  const [searchParams, setSearchParams] = useSearchParams();

  const rawTab = searchParams.get('tab');
  const activeTab: ActionsTab = isActionsTab(rawTab) ? rawTab : 'dashboard';

  function handleTabChange(tab: ActionsTab) {
    setSearchParams(tab === 'dashboard' ? {} : { tab }, { replace: true });
  }

  if (!repoId) return null;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Zap size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Actions</h1>
        <div className="ml-4 flex gap-1">
          {TABS.map((tab) => (
            <button
              key={tab.key}
              onClick={() => handleTabChange(tab.key)}
              className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                activeTab === tab.key
                  ? 'bg-accent/15 text-accent'
                  : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-hidden">
        {activeTab === 'dashboard' && <ActionsDashboard repoId={repoId} />}
        {activeTab === 'history' && <ActionsHistory repoId={repoId} />}
        {activeTab === 'config' && <ActionConfigEditor repoId={repoId} />}
        {activeTab === 'secrets' && <ActionSecretsPanel repoId={repoId} />}
        {activeTab === 'settings' && <ActionsSettings repoId={repoId} />}
      </div>
    </div>
  );
}

export default ActionsPage;
