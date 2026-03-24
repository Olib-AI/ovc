import { useState, type ReactNode } from 'react';
import { ChevronDown, Cloud, CloudOff, LogOut, RefreshCw, GitBranch as GitBranchIcon, Tag, Archive } from 'lucide-react';
import type { BranchInfo, SyncStatusResponse } from '../api/types.ts';
import { useAuth } from '../hooks/useAuth.ts';
import SessionIndicator from './SessionIndicator.tsx';

interface HeaderProps {
  repoName: string;
  currentBranch: string;
  branches: BranchInfo[];
  syncStatus: SyncStatusResponse | undefined;
  onCheckout: (name: string) => void;
  onPush: () => void;
  onPull: () => void;
  isSyncing: boolean;
  /** Optional slot for branch/tag/stash management panel */
  managementPanel?: ReactNode;
}

function Header({
  repoName,
  currentBranch,
  branches,
  syncStatus,
  onCheckout,
  onPush,
  onPull,
  isSyncing,
  managementPanel,
}: HeaderProps) {
  const [showBranchMenu, setShowBranchMenu] = useState(false);
  const [showManagement, setShowManagement] = useState(false);
  const { logout } = useAuth();

  const syncLabel = getSyncLabel(syncStatus?.status);

  return (
    <>
      <header className="flex h-12 items-center justify-between border-b border-border bg-navy-900 px-4">
        <div className="flex items-center gap-4">
          <h1 className="text-sm font-semibold text-text-primary">{repoName}</h1>

          <div className="relative">
            <button
              onClick={() => setShowBranchMenu(!showBranchMenu)}
              aria-label="Switch branch"
              className="flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-xs font-medium text-accent transition-colors hover:border-accent/40"
            >
              <span className="font-mono">{currentBranch || 'detached'}</span>
              <ChevronDown size={12} />
            </button>

            {showBranchMenu && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={() => setShowBranchMenu(false)}
                />
                <div className="absolute left-0 top-full z-20 mt-1 min-w-[180px] max-h-[300px] overflow-y-auto rounded-md border border-border bg-navy-800 py-1 shadow-lg">
                  {branches.map((branch) => (
                    <button
                      key={branch.name}
                      onClick={() => {
                        onCheckout(branch.name);
                        setShowBranchMenu(false);
                      }}
                      className={`flex w-full items-center px-3 py-1.5 text-left text-xs transition-colors ${
                        branch.is_current
                          ? 'bg-accent/10 text-accent'
                          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                      }`}
                    >
                      <span className="font-mono">{branch.name}</span>
                      {branch.is_current && (
                        <span className="ml-auto text-[10px] text-accent">current</span>
                      )}
                    </button>
                  ))}
                  {branches.length === 0 && (
                    <p className="px-3 py-2 text-xs text-text-muted">No branches</p>
                  )}
                </div>
              </>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 text-xs text-text-muted">
            {syncStatus?.status === 'no_remote' ? (
              <CloudOff size={14} />
            ) : (
              <Cloud size={14} className="text-accent" />
            )}
            <span>{syncLabel}</span>
          </div>

          {syncStatus?.status !== 'no_remote' && (
            <div className="flex gap-1">
              <button
                onClick={onPull}
                disabled={isSyncing}
                className="rounded px-2 py-1 text-xs text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary disabled:opacity-40"
                title="Pull"
                aria-label="Pull"
              >
                {isSyncing ? <RefreshCw size={13} className="animate-spin" /> : 'Pull'}
              </button>
              <button
                onClick={onPush}
                disabled={isSyncing}
                className="rounded bg-accent/15 px-2 py-1 text-xs text-accent transition-colors hover:bg-accent/25 disabled:opacity-40"
                title="Push"
                aria-label="Push"
              >
                Push
              </button>
            </div>
          )}

          {managementPanel && (
            <>
              <div className="mx-1 h-4 w-px bg-border" />
              <button
                onClick={() => setShowManagement(!showManagement)}
                className={`flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors ${
                  showManagement
                    ? 'bg-accent/15 text-accent'
                    : 'text-text-muted hover:bg-surface-hover hover:text-text-primary'
                }`}
                title="Branches, Tags & Stash"
                aria-label="Branches, Tags & Stash"
              >
                <GitBranchIcon size={13} />
                <Tag size={11} />
                <Archive size={11} />
              </button>
            </>
          )}

          <div className="mx-1 h-4 w-px bg-border" />
          <SessionIndicator />
          <button
            onClick={logout}
            className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            title="Logout"
            aria-label="Logout"
          >
            <LogOut size={14} />
          </button>
        </div>
      </header>

      {/* Management panel slide-out */}
      {showManagement && managementPanel && (
        <div className="border-b border-border bg-navy-900">
          <div className="flex max-h-[400px] overflow-y-auto">
            {managementPanel}
          </div>
        </div>
      )}
    </>
  );
}

function getSyncLabel(status: string | undefined): string {
  switch (status) {
    case 'in_sync':
      return 'In sync';
    case 'local_ahead':
      return 'Ahead';
    case 'remote_ahead':
      return 'Behind';
    case 'diverged':
      return 'Diverged';
    case 'no_remote':
      return 'No remote';
    default:
      return 'Unknown';
  }
}

export default Header;
