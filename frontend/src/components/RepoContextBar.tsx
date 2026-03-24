import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import {
  ChevronDown,
  Cloud,
  CloudOff,
  LogOut,
  RefreshCw,
  GitBranch as GitBranchIcon,
  Tag,
  Archive,
  Loader2,
} from 'lucide-react';
import {
  useRepo,
  useRepoStatus,
  useSyncStatus,
  useBranches,
  useTags,
  useStash,
  useCheckoutBranch,
  usePushSync,
  usePullSync,
  useCreateBranch,
  useDeleteBranch,
  useMergeBranch,
  useCreateTag,
  useDeleteTag,
  usePushStash,
  usePopStash,
  useApplyStash,
  useDropStash,
  useClearStash,
  useRebase,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import { useAuth } from '../hooks/useAuth.ts';
import BranchList from './BranchList.tsx';
import TagList from './TagList.tsx';
import StashPanel from './StashPanel.tsx';
import SessionIndicator from './SessionIndicator.tsx';

function RepoContextBar() {
  const { repoId } = useParams<{ repoId: string }>();
  const navigate = useNavigate();
  const toast = useToast();
  const { logout } = useAuth();
  const { data: repo, isLoading } = useRepo(repoId);
  const { data: status } = useRepoStatus(repoId);
  const { data: syncStatus } = useSyncStatus(repoId);
  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);
  const { data: stashes } = useStash(repoId);

  const checkoutBranch = useCheckoutBranch(repoId ?? '');
  const pushSync = usePushSync(repoId ?? '');
  const pullSync = usePullSync(repoId ?? '');
  const createBranch = useCreateBranch(repoId ?? '');
  const deleteBranch = useDeleteBranch(repoId ?? '');
  const mergeBranch = useMergeBranch(repoId ?? '');
  const createTag = useCreateTag(repoId ?? '');
  const deleteTag = useDeleteTag(repoId ?? '');
  const pushStash = usePushStash(repoId ?? '');
  const popStash = usePopStash(repoId ?? '');
  const applyStash = useApplyStash(repoId ?? '');
  const dropStash = useDropStash(repoId ?? '');
  const clearStash = useClearStash(repoId ?? '');
  const rebaseMutation = useRebase(repoId ?? '');

  const [showBranchMenu, setShowBranchMenu] = useState(false);
  const [showManagement, setShowManagement] = useState(false);

  if (isLoading || !repo) {
    return (
      <div className="flex h-12 items-center gap-3 border-b border-border bg-navy-900 px-4">
        <span className="text-sm font-semibold text-text-primary">{repoId}</span>
        <Loader2 size={14} className="animate-spin text-text-muted" />
      </div>
    );
  }

  const currentBranch = status?.branch ?? repo.head ?? 'main';
  const isSyncing = pushSync.isPending || pullSync.isPending;
  const syncLabel = getSyncLabel(syncStatus?.status);

  return (
    <>
      <header className="flex h-12 items-center justify-between border-b border-border bg-navy-900 px-4">
        <div className="flex items-center gap-4">
          <h1 className="text-sm font-semibold text-text-primary">{repo.name}</h1>

          {/* Branch switcher */}
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
                  {(branches ?? []).map((branch) => (
                    <button
                      key={branch.name}
                      onClick={() => {
                        checkoutBranch.mutate(branch.name, {
                          onSuccess: () => toast.success('Switched to branch'),
                          onError: (err: Error) => toast.error(err.message),
                        });
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
                  {(!branches || branches.length === 0) && (
                    <p className="px-3 py-2 text-xs text-text-muted">No branches</p>
                  )}
                </div>
              </>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {/* Sync status */}
          <div className="flex items-center gap-1.5 text-xs text-text-muted">
            {syncStatus?.status === 'no_remote' ? (
              <CloudOff size={14} />
            ) : (
              <Cloud size={14} className="text-accent" />
            )}
            <span>{syncLabel}</span>
          </div>

          {/* Push / Pull */}
          {syncStatus?.status !== 'no_remote' && (
            <div className="flex gap-1">
              <button
                onClick={() => {
                  const toastId = toast.progress('Pulling...');
                  pullSync.mutate(undefined, {
                    onSuccess: () => toast.updateToast(toastId, 'success', 'Pulled successfully'),
                    onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
                  });
                }}
                disabled={isSyncing}
                className="rounded px-2 py-1 text-xs text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary disabled:opacity-40"
                title="Pull"
                aria-label="Pull"
              >
                {isSyncing ? <RefreshCw size={13} className="animate-spin" /> : 'Pull'}
              </button>
              <button
                onClick={() => {
                  const toastId = toast.progress('Pushing...');
                  pushSync.mutate(undefined, {
                    onSuccess: () => toast.updateToast(toastId, 'success', 'Pushed successfully'),
                    onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
                  });
                }}
                disabled={isSyncing}
                className="rounded bg-accent/15 px-2 py-1 text-xs text-accent transition-colors hover:bg-accent/25 disabled:opacity-40"
                title="Push"
                aria-label="Push"
              >
                Push
              </button>
            </div>
          )}

          {/* Management panel toggle */}
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
      {showManagement && (
        <div className="border-b border-border bg-navy-900">
          <div className="flex max-h-[400px] overflow-y-auto">
            <div className="flex w-full divide-x divide-border">
              <div className="min-w-0 flex-1 p-2">
                <BranchList
                  branches={branches ?? []}
                  onCreateBranch={(name, startPoint) =>
                    createBranch.mutate({ name, startPoint }, {
                      onSuccess: () => toast.success('Branch created'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onDeleteBranch={(name) =>
                    deleteBranch.mutate(name, {
                      onSuccess: () => toast.success('Branch deleted'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onCheckout={(name) =>
                    checkoutBranch.mutate(name, {
                      onSuccess: () => toast.success('Switched to branch'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onMerge={(target, source) =>
                    mergeBranch.mutate({ target, source }, {
                      onSuccess: () => toast.success('Merged successfully'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onCompare={(name) => navigate(`/repo/${repoId}/pulls/${encodeURIComponent(name)}`)}
                  onRebase={(onto) =>
                    rebaseMutation.mutate(onto, {
                      onSuccess: (result) => {
                        if (result.conflict_files && result.conflict_files.length > 0) {
                          toast.warning(
                            `Rebase completed with conflicts: ${result.conflict_files.join(', ')}`,
                          );
                        } else {
                          toast.success('Rebased successfully');
                        }
                      },
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  isCreating={createBranch.isPending}
                  isDeleting={deleteBranch.isPending}
                />
              </div>
              <div className="min-w-0 flex-1 p-2">
                <TagList
                  tags={tags ?? []}
                  onCreateTag={(name, message, commitId) =>
                    createTag.mutate(
                      { name, message, commitId },
                      {
                        onSuccess: () => toast.success(`Tag "${name}" created`),
                        onError: (err: Error) => toast.error(err.message),
                      },
                    )
                  }
                  onDeleteTag={(name) =>
                    deleteTag.mutate(name, {
                      onSuccess: () => toast.success('Tag deleted'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  isCreating={createTag.isPending}
                />
              </div>
              <div className="min-w-0 flex-1 p-2">
                <StashPanel
                  repoId={repoId!}
                  stashes={stashes ?? []}
                  onPush={(msg) =>
                    pushStash.mutate(msg, {
                      onSuccess: () => toast.success('Stashed changes'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onPop={(idx) =>
                    popStash.mutate(idx, {
                      onSuccess: () => toast.success('Stash popped'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onApply={(idx) =>
                    applyStash.mutate(idx, {
                      onSuccess: () => toast.success('Stash applied successfully'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onDrop={(idx) =>
                    dropStash.mutate(idx, {
                      onSuccess: () => toast.success('Stash entry dropped'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  onClear={() =>
                    clearStash.mutate(undefined, {
                      onSuccess: () => toast.success('All stashes cleared'),
                      onError: (err: Error) => toast.error(err.message),
                    })
                  }
                  isPushing={pushStash.isPending}
                  isClearing={clearStash.isPending}
                  isMutating={popStash.isPending || applyStash.isPending || dropStash.isPending}
                />
              </div>
            </div>
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

export default RepoContextBar;
