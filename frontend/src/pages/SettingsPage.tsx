import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { Settings, Trash2, RefreshCw, HardDrive, AlertTriangle, Package, Plus, X, User, Link, FolderOpen, Pin, Info, Archive } from 'lucide-react';
import {
  useRepo,
  useDeleteRepo,
  useGc,
  useSyncStatus,
  useSubmodules,
  useAddSubmodule,
  useDeleteSubmodule,
  useRemotes,
  useAddRemote,
  useDeleteRemote,
  useRepoConfig,
  useUpdateRepoConfig,
} from '../hooks/useRepo.ts';
import { usePushSync, usePullSync } from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import SyncPanel from '../components/SyncPanel.tsx';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import ArchiveDialog from '../components/ArchiveDialog.tsx';
import GitIntegrationPanel from '../components/GitIntegrationPanel.tsx';
import type { GcResponse } from '../api/types.ts';

function SettingsPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Settings \u2014 OVC`);
  const navigate = useNavigate();
  const { data: repo, isLoading } = useRepo(repoId);
  const { data: syncStatus } = useSyncStatus(repoId);
  const deleteRepo = useDeleteRepo();
  const gcMutation = useGc(repoId ?? '');
  const pushSync = usePushSync(repoId ?? '');
  const pullSync = usePullSync(repoId ?? '');

  const { data: submodules } = useSubmodules(repoId);
  const addSubmoduleMutation = useAddSubmodule(repoId ?? '');
  const deleteSubmoduleMutation = useDeleteSubmodule(repoId ?? '');
  const { data: remotes } = useRemotes(repoId);
  const addRemoteMutation = useAddRemote(repoId ?? '');
  const deleteRemoteMutation = useDeleteRemote(repoId ?? '');
  const { data: repoConfig } = useRepoConfig(repoId);
  const updateRepoConfigMutation = useUpdateRepoConfig(repoId ?? '');
  const toast = useToast();

  const [confirmDelete, setConfirmDelete] = useState(false);
  const [gcResult, setGcResult] = useState<GcResponse | null>(null);
  const [showAddSubmodule, setShowAddSubmodule] = useState(false);
  const [subName, setSubName] = useState('');
  const [subPath, setSubPath] = useState('');
  const [subUrl, setSubUrl] = useState('');
  const [showArchiveDialog, setShowArchiveDialog] = useState(false);

  // Identity & Config form state — pre-filled when config loads.
  // Track whether the form has been initialised from the server data so we
  // only overwrite user input once (on first load), not on every refetch.
  const [configInitialised, setConfigInitialised] = useState(false);
  const [configUserName, setConfigUserName] = useState('');
  const [configUserEmail, setConfigUserEmail] = useState('');
  const [configDefaultBranch, setConfigDefaultBranch] = useState('');

  if (repoConfig && !configInitialised) {
    setConfigInitialised(true);
    setConfigUserName(repoConfig.user_name);
    setConfigUserEmail(repoConfig.user_email);
    setConfigDefaultBranch(repoConfig.default_branch);
  }

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading settings..." />;
  }

  if (!repo) return null;

  function handleSaveConfig() {
    updateRepoConfigMutation.mutate(
      {
        user_name: configUserName.trim() || undefined,
        user_email: configUserEmail.trim() || undefined,
        default_branch: configDefaultBranch.trim() || undefined,
      },
      {
        onSuccess: () => toast.success('Configuration saved'),
        onError: (err: Error) => toast.error(err.message),
      },
    );
  }

  function handleDelete() {
    if (!repoId) return;
    deleteRepo.mutate(repoId, {
      onSuccess: () => navigate('/'),
      onError: (err: Error) => toast.error(err.message),
    });
  }

  function handleGc() {
    gcMutation.mutate(undefined, {
      onSuccess: (result) => setGcResult(result),
      onError: (err: Error) => toast.error(err.message),
    });
  }

  function handleAddSubmodule() {
    if (!subName.trim() || !subPath.trim() || !subUrl.trim()) return;
    addSubmoduleMutation.mutate(
      { name: subName.trim(), path: subPath.trim(), url: subUrl.trim() },
      {
        onSuccess: () => {
          toast.success('Submodule added');
          setSubName('');
          setSubPath('');
          setSubUrl('');
          setShowAddSubmodule(false);
        },
        onError: (err: Error) => toast.error(err.message),
      },
    );
  }

  function handleDeleteSubmodule(name: string) {
    deleteSubmoduleMutation.mutate(name, {
      onSuccess: () => toast.success('Submodule removed'),
      onError: (err: Error) => toast.error(err.message),
    });
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Settings size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Repository Settings</h1>
      </div>

      <div className="mx-auto max-w-xl space-y-6 p-6">
        {/* Identity & Config */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center gap-2 mb-3">
            <User size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Identity &amp; Config</h2>
          </div>
          <div className="space-y-3">
            <div>
              <label className="mb-1 block text-xs text-text-muted" htmlFor="config-user-name">
                Author Name
              </label>
              <input
                id="config-user-name"
                value={configUserName}
                onChange={(e) => setConfigUserName(e.target.value)}
                placeholder="Your Name"
                className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-text-muted" htmlFor="config-user-email">
                Author Email
              </label>
              <input
                id="config-user-email"
                type="email"
                value={configUserEmail}
                onChange={(e) => setConfigUserEmail(e.target.value)}
                placeholder="you@example.com"
                className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-text-muted" htmlFor="config-default-branch">
                Default Branch
              </label>
              <input
                id="config-default-branch"
                value={configDefaultBranch}
                onChange={(e) => setConfigDefaultBranch(e.target.value)}
                placeholder="main"
                className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
            </div>
            <button
              onClick={handleSaveConfig}
              disabled={updateRepoConfigMutation.isPending}
              className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
            >
              {updateRepoConfigMutation.isPending ? (
                <RefreshCw size={12} className="animate-spin" />
              ) : null}
              {updateRepoConfigMutation.isPending ? 'Saving...' : 'Save'}
            </button>
          </div>
        </div>

        {/* Repo Info */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <h2 className="mb-3 text-sm font-semibold text-text-primary">Repository Info</h2>
          <div className="space-y-2 text-xs">
            <div className="flex justify-between">
              <span className="text-text-muted">Name</span>
              <span className="font-mono text-text-secondary">{repo.name}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">Path</span>
              <span className="max-w-[250px] truncate font-mono text-text-secondary" title={repo.path}>
                {repo.path}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">HEAD</span>
              <span className="font-mono text-text-secondary">{repo.head || 'empty'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">Commits</span>
              <span className="text-text-secondary">{repo.repo_stats.total_commits}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">Branches</span>
              <span className="text-text-secondary">{repo.repo_stats.total_branches}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">Tags</span>
              <span className="text-text-secondary">{repo.repo_stats.total_tags}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-muted">Tracked Files</span>
              <span className="text-text-secondary">{repo.repo_stats.tracked_files}</span>
            </div>
          </div>

          <div className="mt-3 border-t border-border pt-3">
            <button
              onClick={() => setShowArchiveDialog(true)}
              className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
            >
              <Archive size={13} />
              Export Archive...
            </button>
          </div>
        </div>

        {/* Dependency Updates */}
        {repoId && (
          <div className="rounded-lg border border-border bg-navy-900 p-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Package size={16} className="text-accent" />
                <h2 className="text-sm font-semibold text-text-primary">Dependencies</h2>
              </div>
              <a
                href={`/repo/${repoId}/dependencies`}
                className="flex items-center gap-1 text-xs text-accent transition-colors hover:text-accent-light"
              >
                View dependency management &rarr;
              </a>
            </div>
          </div>
        )}

        {/* Submodules */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2">
              <Package size={16} className="text-accent" />
              <h2 className="text-sm font-semibold text-text-primary">Submodules</h2>
            </div>
            <button
              onClick={() => setShowAddSubmodule(!showAddSubmodule)}
              className="flex items-center gap-1 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
            >
              {showAddSubmodule ? <X size={11} /> : <Plus size={11} />}
              {showAddSubmodule ? 'Cancel' : 'Add'}
            </button>
          </div>

          {showAddSubmodule && (
            <div className="mb-3 space-y-2 rounded border border-border bg-navy-950 p-3">
              <input
                value={subName}
                onChange={(e) => setSubName(e.target.value)}
                placeholder="Name"
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
              <input
                value={subPath}
                onChange={(e) => setSubPath(e.target.value)}
                placeholder="Path"
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
              <input
                value={subUrl}
                onChange={(e) => setSubUrl(e.target.value)}
                placeholder="URL"
                className="w-full rounded border border-border bg-navy-900 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
              <button
                onClick={handleAddSubmodule}
                disabled={!subName.trim() || !subPath.trim() || !subUrl.trim() || addSubmoduleMutation.isPending}
                className="rounded bg-accent px-3 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
              >
                {addSubmoduleMutation.isPending ? 'Adding...' : 'Add Submodule'}
              </button>
            </div>
          )}

          {/* Info banner about submodule checkout */}
          <div className="mb-3 flex items-start gap-2 rounded border border-status-modified/20 bg-status-modified/5 px-3 py-2">
            <Info size={13} className="mt-0.5 flex-shrink-0 text-status-modified" />
            <p className="text-[11px] text-text-secondary leading-relaxed">
              Submodule checkout is not yet available. Configuration is stored for future use.
            </p>
          </div>

          {submodules && submodules.length > 0 ? (
            <div className="space-y-2">
              {submodules.map((sub) => {
                const isConfigured = !sub.status || sub.status === 'configured';
                return (
                  <div
                    key={sub.name}
                    className="rounded border border-border bg-navy-950 px-3 py-2.5"
                  >
                    {/* Name row with status badge */}
                    <div className="flex items-center justify-between gap-2 mb-1.5">
                      <p className="text-xs font-semibold text-text-primary">{sub.name}</p>
                      <div className="flex items-center gap-2">
                        <span className={`rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide ${
                          isConfigured
                            ? 'bg-amber-500/15 text-amber-400'
                            : 'bg-green-500/15 text-green-400'
                        }`}>
                          {isConfigured ? 'Configured' : 'Active'}
                        </span>
                        <button
                          onClick={() => handleDeleteSubmodule(sub.name)}
                          disabled={deleteSubmoduleMutation.isPending}
                          className="flex-shrink-0 rounded p-1 text-text-muted transition-colors hover:bg-status-deleted/10 hover:text-status-deleted disabled:opacity-50"
                          title="Remove submodule"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    </div>

                    {/* Path row */}
                    <div className="flex items-center gap-1.5 mb-1">
                      <FolderOpen size={11} className="flex-shrink-0 text-text-muted" />
                      <span
                        className="truncate font-mono text-[11px] text-text-secondary"
                        title={sub.path}
                      >
                        {sub.path}
                      </span>
                    </div>

                    {/* URL row */}
                    <div className="flex items-center gap-1.5 mb-1">
                      <Link size={11} className="flex-shrink-0 text-text-muted" />
                      <span
                        className="truncate font-mono text-[11px] text-accent/70"
                        title={sub.url}
                      >
                        {sub.url}
                      </span>
                    </div>

                    {/* Pinned sequence */}
                    {sub.pinned_sequence > 0 && (
                      <div className="flex items-center gap-1.5">
                        <Pin size={11} className="flex-shrink-0 text-text-muted" />
                        <span className="text-[11px] text-text-muted">
                          Pinned at sequence <span className="font-mono text-text-secondary">{sub.pinned_sequence}</span>
                        </span>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="text-xs text-text-muted">No submodules configured</p>
          )}
        </div>

        {/* Sync */}
        {syncStatus && (
          <SyncPanel
            syncStatus={syncStatus}
            onPush={() =>
              pushSync.mutate(undefined, {
                onSuccess: () => toast.success('Push sync completed'),
                onError: (err: Error) => toast.error(err.message),
              })
            }
            onPull={() =>
              pullSync.mutate(undefined, {
                onSuccess: () => toast.success('Pull sync completed'),
                onError: (err: Error) => toast.error(err.message),
              })
            }
            isPushing={pushSync.isPending}
            isPulling={pullSync.isPending}
            remotes={remotes}
            onAddRemote={(name, url, backendType) => {
              addRemoteMutation.mutate(
                { name, url, backendType },
                {
                  onSuccess: () => toast.success(`Remote "${name}" added`),
                  onError: (err: Error) => toast.error(err.message),
                },
              );
            }}
            isAddingRemote={addRemoteMutation.isPending}
            onDeleteRemote={(name) => {
              deleteRemoteMutation.mutate(name, {
                onSuccess: () => toast.success(`Remote "${name}" removed`),
                onError: (err: Error) => toast.error(err.message),
              });
            }}
            isDeletingRemote={deleteRemoteMutation.isPending}
          />
        )}

        {/* Garbage Collection */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center gap-2 mb-3">
            <HardDrive size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Garbage Collection</h2>
          </div>
          <p className="mb-3 text-xs text-text-muted">
            Remove unreachable objects and optimize storage
          </p>

          {gcResult && (
            <div className="mb-3 rounded border border-status-added/30 bg-diff-add-bg/30 p-3 text-xs">
              <p className="font-medium text-status-added mb-1">GC Complete</p>
              <div className="space-y-1 text-text-secondary">
                <p>Objects: {gcResult.objects_before} → {gcResult.objects_after} ({gcResult.objects_removed} removed)</p>
                <p>
                  Size: {formatBytes(gcResult.bytes_before)} → {formatBytes(gcResult.bytes_after)} ({formatBytes(gcResult.bytes_freed)} freed)
                </p>
              </div>
            </div>
          )}

          <button
            onClick={handleGc}
            disabled={gcMutation.isPending}
            className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
          >
            {gcMutation.isPending ? (
              <RefreshCw size={13} className="animate-spin" />
            ) : (
              <HardDrive size={13} />
            )}
            Run GC
          </button>
        </div>

        {/* Git Integration */}
        {repoId && <GitIntegrationPanel repoId={repoId} />}

        {/* Danger Zone */}
        <div className="rounded-lg border border-status-deleted/30 bg-diff-del-bg/20 p-4">
          <div className="flex items-center gap-2 mb-3">
            <AlertTriangle size={16} className="text-status-deleted" />
            <h2 className="text-sm font-semibold text-status-deleted">Danger Zone</h2>
          </div>

          {!confirmDelete ? (
            <button
              onClick={() => setConfirmDelete(true)}
              className="flex items-center gap-1.5 rounded border border-status-deleted/30 px-3 py-1.5 text-xs text-status-deleted transition-colors hover:bg-status-deleted/10"
            >
              <Trash2 size={13} />
              Delete Repository
            </button>
          ) : (
            <div>
              <p className="mb-2 text-xs text-text-secondary">
                This will permanently delete <span className="font-bold text-text-primary">{repo.name}</span>. This action cannot be undone.
              </p>
              <div className="flex gap-2">
                <button
                  onClick={handleDelete}
                  disabled={deleteRepo.isPending}
                  className="rounded bg-status-deleted px-3 py-1.5 text-xs font-medium text-white hover:opacity-90 disabled:opacity-50"
                >
                  {deleteRepo.isPending ? 'Deleting...' : 'Confirm Delete'}
                </button>
                <button
                  onClick={() => setConfirmDelete(false)}
                  className="rounded px-3 py-1.5 text-xs text-text-muted hover:text-text-primary"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      </div>

      {showArchiveDialog && repoId && (
        <ArchiveDialog
          repoId={repoId}
          repoName={repo.name}
          onClose={() => setShowArchiveDialog(false)}
        />
      )}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export default SettingsPage;
