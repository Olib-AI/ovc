import { useState } from 'react';
import { Cloud, CloudOff, Upload, Download, RefreshCw, Plus, Trash2, X, Globe, HardDrive } from 'lucide-react';
import type { SyncStatusResponse, RemoteInfo } from '../api/types.ts';

interface SyncPanelProps {
  syncStatus: SyncStatusResponse;
  onPush: () => void;
  onPull: () => void;
  isPushing: boolean;
  isPulling: boolean;
  remotes?: RemoteInfo[];
  onAddRemote?: (name: string, url: string, backendType: string) => void;
  isAddingRemote?: boolean;
  onDeleteRemote?: (name: string) => void;
  isDeletingRemote?: boolean;
}

function SyncPanel({
  syncStatus,
  onPush,
  onPull,
  isPushing,
  isPulling,
  remotes,
  onAddRemote,
  isAddingRemote,
  onDeleteRemote,
  isDeletingRemote,
}: SyncPanelProps) {
  const isNoRemote = syncStatus.status === 'no_remote';
  const isSyncing = isPushing || isPulling;

  const [showAddForm, setShowAddForm] = useState(false);
  const [remoteName, setRemoteName] = useState('');
  const [remoteUrl, setRemoteUrl] = useState('');
  const [backendType, setBackendType] = useState('local');
  const [confirmDeleteRemote, setConfirmDeleteRemote] = useState<string | null>(null);

  function handleAddRemote() {
    if (!remoteName.trim() || !remoteUrl.trim() || !onAddRemote) return;
    onAddRemote(remoteName.trim(), remoteUrl.trim(), backendType);
    setRemoteName('');
    setRemoteUrl('');
    setBackendType('local');
    setShowAddForm(false);
  }

  return (
    <div className="rounded-lg border border-border bg-navy-800/50 p-4">
      <div className="flex items-center gap-2 mb-3">
        {isNoRemote ? (
          <CloudOff size={18} className="text-text-muted" />
        ) : (
          <Cloud size={18} className="text-accent" />
        )}
        <h3 className="text-sm font-semibold text-text-primary">Cloud Sync</h3>
      </div>

      {/* Remote Management */}
      {onAddRemote && (
        <div className="mb-4">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-medium text-text-secondary">Remotes</span>
            <button
              onClick={() => setShowAddForm(!showAddForm)}
              className="flex items-center gap-1 rounded border border-border bg-surface px-2 py-0.5 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
            >
              {showAddForm ? <X size={10} /> : <Plus size={10} />}
              {showAddForm ? 'Cancel' : 'Add'}
            </button>
          </div>

          {showAddForm && (
            <div className="mb-3 space-y-2 rounded border border-border bg-navy-950 p-3">
              <input
                value={remoteName}
                onChange={(e) => setRemoteName(e.target.value)}
                placeholder="Remote name (e.g. origin)"
                aria-label="Remote name"
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
              <input
                value={remoteUrl}
                onChange={(e) => setRemoteUrl(e.target.value)}
                placeholder="Remote URL"
                aria-label="Remote URL"
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
              <select
                value={backendType}
                onChange={(e) => setBackendType(e.target.value)}
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary focus:border-accent focus:outline-none"
              >
                <option value="local">Local</option>
                <option value="gcs">Google Cloud Storage</option>
              </select>
              <button
                onClick={handleAddRemote}
                disabled={!remoteName.trim() || !remoteUrl.trim() || isAddingRemote}
                className="rounded bg-accent px-3 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
              >
                {isAddingRemote ? 'Adding...' : 'Add Remote'}
              </button>
            </div>
          )}

          {remotes && remotes.length > 0 ? (
            <div className="space-y-1.5">
              {remotes.map((remote) => (
                <div
                  key={remote.name}
                  className="flex items-center justify-between rounded border border-border bg-navy-950 px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs font-medium text-text-primary">{remote.name}</span>
                      <span className="flex items-center gap-0.5 rounded bg-surface px-1.5 py-0.5 text-[10px] text-text-muted">
                        {remote.backend_type === 'gcs' ? (
                          <Globe size={9} />
                        ) : (
                          <HardDrive size={9} />
                        )}
                        {remote.backend_type}
                      </span>
                    </div>
                    <p className="mt-0.5 truncate text-[11px] font-mono text-text-muted" title={remote.url}>
                      {remote.url}
                    </p>
                  </div>
                  {onDeleteRemote && (
                    <button
                      onClick={() => setConfirmDeleteRemote(remote.name)}
                      disabled={isDeletingRemote}
                      className="ml-2 flex-shrink-0 rounded p-1 text-text-muted transition-colors hover:bg-status-deleted/10 hover:text-status-deleted disabled:opacity-50"
                      title="Remove remote"
                    >
                      <Trash2 size={12} />
                    </button>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-muted">
              No remotes configured. Add a remote to enable cloud sync.
            </p>
          )}

          <div className="mt-3 border-t border-border pt-3" />
        </div>
      )}

      <div className="space-y-2 text-xs">
        <div className="flex justify-between">
          <span className="text-text-muted">Status</span>
          <SyncStatusBadge status={syncStatus.status} />
        </div>

        {syncStatus.remote && (
          <div className="flex justify-between">
            <span className="text-text-muted">Remote</span>
            <span className="font-mono text-text-secondary">{syncStatus.remote}</span>
          </div>
        )}

        {syncStatus.version !== null && syncStatus.version !== undefined && (
          <div className="flex justify-between">
            <span className="text-text-muted">Version</span>
            <span className="font-mono text-text-secondary">{syncStatus.version}</span>
          </div>
        )}
      </div>

      {/* Confirm remote deletion */}
      {confirmDeleteRemote && onDeleteRemote && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="fixed inset-0 bg-navy-950/70" onClick={() => setConfirmDeleteRemote(null)} />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-4 shadow-2xl">
            <p className="mb-4 text-sm text-text-primary">
              Remove remote <span className="font-mono text-accent">{confirmDeleteRemote}</span>?
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDeleteRemote(null)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onDeleteRemote(confirmDeleteRemote);
                  setConfirmDeleteRemote(null);
                }}
                disabled={isDeletingRemote}
                className="rounded bg-status-deleted/20 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/30 disabled:opacity-50"
              >
                Remove
              </button>
            </div>
          </div>
        </div>
      )}

      {!isNoRemote && (
        <div className="mt-4 flex gap-2">
          <button
            onClick={onPull}
            disabled={isSyncing}
            className="flex flex-1 items-center justify-center gap-1.5 rounded border border-border bg-surface py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-40"
          >
            {isPulling ? (
              <RefreshCw size={13} className="animate-spin" />
            ) : (
              <Download size={13} />
            )}
            Pull
          </button>
          <button
            onClick={onPush}
            disabled={isSyncing}
            className="flex flex-1 items-center justify-center gap-1.5 rounded bg-accent py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-40"
          >
            {isPushing ? (
              <RefreshCw size={13} className="animate-spin" />
            ) : (
              <Upload size={13} />
            )}
            Push
          </button>
        </div>
      )}
    </div>
  );
}

function SyncStatusBadge({ status }: { status: string }) {
  let className: string;
  let label: string;

  switch (status) {
    case 'in_sync':
      className = 'text-status-added';
      label = 'In Sync';
      break;
    case 'local_ahead':
      className = 'text-status-modified';
      label = 'Ahead';
      break;
    case 'remote_ahead':
      className = 'text-accent';
      label = 'Behind';
      break;
    case 'diverged':
      className = 'text-status-deleted';
      label = 'Diverged';
      break;
    case 'no_remote':
      className = 'text-text-muted';
      label = 'No Remote';
      break;
    default:
      className = 'text-text-muted';
      label = status;
  }

  return <span className={`font-medium ${className}`}>{label}</span>;
}

export default SyncPanel;
