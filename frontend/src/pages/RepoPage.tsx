import { useState, useCallback, useEffect, useRef } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import axios from 'axios';
import Header from '../components/Header.tsx';
import FileTree from '../components/FileTree.tsx';
import FileViewer from '../components/FileViewer.tsx';
import StatusPanel from '../components/StatusPanel.tsx';
import ChangesDiffView from '../components/ChangesDiffView.tsx';
import BranchList from '../components/BranchList.tsx';
import StashPanel from '../components/StashPanel.tsx';
import TagList from '../components/TagList.tsx';
import CommitForm from '../components/CommitForm.tsx';
import MergePanel from '../components/MergePanel.tsx';
import UnlockModal from '../components/UnlockModal.tsx';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import { useQueryClient } from '@tanstack/react-query';
import {
  useRepo,
  useRepoStatus,
  useStagedDiff,
  useUnstagedDiff,
  useBranches,
  useTags,
  useStash,
  useSyncStatus,
  useCheckoutBranch,
  useStageFiles,
  useUnstageFiles,
  useRestoreFiles,
  useCreateCommit,
  useCommitLog,
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
  usePushSync,
  usePullSync,
  useUnlockRepo,
  useRebase,
  useCleanFiles,
  useGc,
  useResetCommit,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import { useCommandPalette } from '../contexts/CommandPaletteContext.tsx';
import { useTheme } from '../contexts/ThemeContext.tsx';
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut.ts';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import type { MergeResponse } from '../api/types.ts';
import type { PaletteCommand } from '../contexts/CommandPaletteContext.tsx';
import { FolderTree, GitPullRequestDraft, History, X, FilePlus, Upload, FolderPlus, RefreshCw } from 'lucide-react';
import type { ReactNode } from 'react';
import {
  usePutBlob,
  useUploadFiles,
  useCreateDirectory,
} from '../hooks/useRepo.ts';

function formatGcBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

function RepoPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Files \u2014 OVC`);
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileHistoryRequested, setFileHistoryRequested] = useState(false);
  const [highlightLine, setHighlightLine] = useState<number | undefined>(undefined);
  const [selectedChangedFile, setSelectedChangedFile] = useState<string | null>(null);
  const [activeView, setActiveView] = useState<'files' | 'status'>('files');
  const [mergeResult, setMergeResult] = useState<MergeResponse | null>(null);
  const [showUnlock, setShowUnlock] = useState(false);
  const [unlockError, setUnlockError] = useState<string | null>(null);
  const [showCreateBranch, setShowCreateBranch] = useState(false);
  const [browseRef, setBrowseRef] = useState<string | null>(null);
  const [showNewFileModal, setShowNewFileModal] = useState(false);
  const [showUploadModal, setShowUploadModal] = useState(false);
  const [showNewFolderModal, setShowNewFolderModal] = useState(false);
  const toast = useToast();
  const { toggleTheme } = useTheme();
  const queryClient = useQueryClient();

  // Consume ?file=, ?ref=, ?line=, and ?view= search params
  useEffect(() => {
    const fileParam = searchParams.get('file');
    const refParam = searchParams.get('ref');
    const lineParam = searchParams.get('line');
    const viewParam = searchParams.get('view');
    let changed = false;

    if (viewParam === 'changes') {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setActiveView('status');
      changed = true;
    }

    if (fileParam) {
      setSelectedFile(fileParam);
      setActiveView('files');
      changed = true;
    }

    if (lineParam) {
      const parsed = parseInt(lineParam, 10);
      if (!Number.isNaN(parsed) && parsed > 0) {
        setHighlightLine(parsed);
      }
      changed = true;
    }

    if (refParam) {
      setBrowseRef(refParam);
      setActiveView('files');
      changed = true;
    }

    if (changed) {
      setSearchParams((prev) => {
        const next = new URLSearchParams(prev);
        next.delete('file');
        next.delete('ref');
        next.delete('line');
        next.delete('view');
        return next;
      }, { replace: true });
    }
  }, [searchParams, setSearchParams]);
  const { registerCommands, unregisterCommands } = useCommandPalette();

  const { data: repo, isLoading: repoLoading, error: repoError } = useRepo(repoId);
  const { data: status } = useRepoStatus(repoId);
  const { data: stagedDiff, isLoading: stagedDiffLoading } = useStagedDiff(repoId);
  const { data: unstagedDiff, isLoading: unstagedDiffLoading } = useUnstagedDiff(repoId);
  const diffLoading = stagedDiffLoading || unstagedDiffLoading;
  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);
  const { data: stashes } = useStash(repoId);
  const { data: syncStatus } = useSyncStatus(repoId);

  const { data: log } = useCommitLog(repoId, 1);

  const checkoutBranch = useCheckoutBranch(repoId ?? '');
  const stageFiles = useStageFiles(repoId ?? '');
  const unstageFiles = useUnstageFiles(repoId ?? '');
  const restoreFilesMutation = useRestoreFiles(repoId ?? '');
  const createCommit = useCreateCommit(repoId ?? '');
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
  const pushSync = usePushSync(repoId ?? '');
  const pullSync = usePullSync(repoId ?? '');
  const rebaseMutation = useRebase(repoId ?? '');
  const cleanFilesMutation = useCleanFiles(repoId ?? '');
  const gcMutation = useGc(repoId ?? '');
  const resetCommitMutation = useResetCommit(repoId ?? '');
  const unlockRepo = useUnlockRepo();
  const putBlobMutation = usePutBlob(repoId ?? '');
  const uploadFilesMutation = useUploadFiles(repoId ?? '');
  const createDirectoryMutation = useCreateDirectory(repoId ?? '');

  // B1: Escape to close/deselect
  useKeyboardShortcut('Escape', () => {
    if (mergeResult) {
      setMergeResult(null);
    } else if (selectedFile) {
      setSelectedFile(null);
    } else if (selectedChangedFile) {
      setSelectedChangedFile(null);
    }
  });

  // ⌘+Shift+P — Push to remote
  useKeyboardShortcut('p', () => {
    const toastId = toast.progress('Pushing...');
    pushSync.mutate(undefined, {
      onSuccess: () => toast.updateToast(toastId, 'success', 'Pushed successfully'),
      onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
    });
  }, { meta: true, shift: true });

  // ⌘+Shift+L — Pull from remote
  useKeyboardShortcut('l', () => {
    const toastId = toast.progress('Pulling...');
    pullSync.mutate(undefined, {
      onSuccess: () => toast.updateToast(toastId, 'success', 'Pulled successfully'),
      onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
    });
  }, { meta: true, shift: true });

  // B2: Register commands for command palette
  useEffect(() => {
    if (!repoId) return;

    const commands: PaletteCommand[] = [
      // Git operations
      {
        id: 'repo-commit',
        label: 'Commit staged changes',
        category: 'Git',
        shortcut: '⌘+Enter',
        action: () => setActiveView('status'),
      },
      {
        id: 'repo-push',
        label: 'Push to remote',
        category: 'Git',
        shortcut: '⌘+Shift+P',
        action: () => {
          const toastId = toast.progress('Pushing to remote...');
          pushSync.mutate(undefined, {
            onSuccess: () => toast.updateToast(toastId, 'success', 'Pushed successfully'),
            onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
          });
        },
      },
      {
        id: 'repo-pull',
        label: 'Pull from remote',
        category: 'Git',
        shortcut: '⌘+Shift+L',
        action: () => {
          const toastId = toast.progress('Pulling from remote...');
          pullSync.mutate(undefined, {
            onSuccess: () => toast.updateToast(toastId, 'success', 'Pulled successfully'),
            onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
          });
        },
      },
      {
        id: 'repo-stage-all',
        label: 'Stage all files',
        category: 'Git',
        action: () => {
          stageFiles.mutate(['.'], {
            onSuccess: () => toast.success('All files staged'),
            onError: (err: Error) => toast.error(err.message),
          });
        },
      },
      {
        id: 'repo-unstage-all',
        label: 'Unstage all files',
        category: 'Git',
        action: () => {
          unstageFiles.mutate(['.'], {
            onSuccess: () => toast.success('All files unstaged'),
            onError: (err: Error) => toast.error(err.message),
          });
        },
      },
      {
        id: 'repo-stash-push',
        label: 'Stash changes',
        category: 'Git',
        action: () => {
          pushStash.mutate(undefined, {
            onSuccess: () => toast.success('Stashed changes'),
            onError: (err: Error) => toast.error(err.message),
          });
        },
      },
      {
        id: 'repo-stash-pop',
        label: 'Pop latest stash',
        category: 'Git',
        action: () => {
          popStash.mutate(0, {
            onSuccess: () => toast.success('Stash popped'),
            onError: (err: Error) => toast.error(err.message),
          });
        },
      },
      {
        id: 'repo-gc',
        label: 'Run garbage collection',
        category: 'Git',
        action: () => {
          const toastId = toast.progress('Running garbage collection...');
          gcMutation.mutate(undefined, {
            onSuccess: (result) =>
              toast.updateToast(
                toastId,
                'success',
                `GC complete: removed ${result.objects_removed} objects, freed ${formatGcBytes(result.bytes_freed)}`,
              ),
            onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
          });
        },
      },
      // Branch
      {
        id: 'repo-create-branch',
        label: 'Create branch',
        category: 'Branch',
        action: () => {
          setActiveView('files');
          setShowCreateBranch(true);
        },
      },
      {
        id: 'repo-switch-branch',
        label: 'Switch branch',
        category: 'Branch',
        action: () => {
          // Focus the branch dropdown in the Header by clicking the element with
          // the data-branch-dropdown attribute, falling back to the first button
          // in the header that contains the current branch name.
          const el = document.querySelector<HTMLElement>('[data-branch-dropdown]');
          if (el) {
            el.click();
          }
        },
      },
      // Navigation (repo-specific views only; global nav is handled by Sidebar)
      {
        id: 'repo-switch-files',
        label: 'Switch to Files',
        category: 'Navigation',
        action: () => setActiveView('files'),
      },
      {
        id: 'repo-switch-changes',
        label: 'Switch to Changes',
        category: 'Navigation',
        action: () => setActiveView('status'),
      },
      {
        id: 'repo-diff',
        label: 'Navigate to Diff',
        category: 'Navigation',
        action: () => navigate(`/repo/${repoId}/diff`),
      },
      {
        id: 'repo-blame',
        label: 'Navigate to Blame',
        category: 'Navigation',
        action: () => navigate(`/repo/${repoId}/blame`),
      },
      {
        id: 'repo-toggle-theme',
        label: 'Toggle theme',
        category: 'Navigation',
        action: toggleTheme,
      },
    ];

    registerCommands(commands);
    const ids = commands.map((c) => c.id);
    return () => unregisterCommands(ids);
  }, [repoId, registerCommands, unregisterCommands, navigate, toast, pushSync, pullSync, pushStash, popStash, gcMutation, stageFiles, unstageFiles, toggleTheme]);

  // Register branch palette entries separately so they update whenever the
  // branch list changes without forcing all other commands to re-register.
  useEffect(() => {
    if (!repoId || !branches || branches.length === 0) return;

    const branchCommands: PaletteCommand[] = branches.map((b) => ({
      id: `branch-checkout-${b.name}`,
      label: b.name,
      category: 'Branch',
      type: 'branch' as const,
      action: () => {
        checkoutBranch.mutate(b.name, {
          onSuccess: () => toast.success(`Switched to ${b.name}`),
          onError: (err: Error) => toast.error(err.message),
        });
      },
    }));

    registerCommands(branchCommands);
    const ids = branchCommands.map((c) => c.id);
    return () => unregisterCommands(ids);
  }, [repoId, branches, registerCommands, unregisterCommands, checkoutBranch, toast]);

  const handleMerge = useCallback(
    (target: string, source: string) => {
      mergeBranch.mutate(
        { target, source },
        {
          onSuccess: (result) => {
            setMergeResult(result);
            toast.success('Merged');
          },
          onError: (err: Error) => toast.error(err.message),
        },
      );
    },
    [mergeBranch, toast],
  );

  const handleUnlock = useCallback(
    (password: string) => {
      if (!repoId) return;
      setUnlockError(null);
      unlockRepo.mutate(
        { id: repoId, password },
        {
          onSuccess: () => {
            setShowUnlock(false);
            void queryClient.invalidateQueries({ queryKey: ['repo', repoId] });
          },
          onError: (err) => {
            if (axios.isAxiosError(err)) {
              const data = err.response?.data as
                | { error?: { message?: string } | string; message?: string }
                | undefined;
              const msg =
                (typeof data?.error === 'object' ? data.error.message : undefined) ??
                (typeof data?.error === 'string' ? data.error : undefined) ??
                data?.message ??
                'Invalid password';
              setUnlockError(msg);
            } else {
              setUnlockError('Failed to unlock');
            }
          },
        },
      );
    },
    [repoId, unlockRepo, queryClient],
  );

  // Check if we need to unlock
  const needsUnlock =
    repoError &&
    axios.isAxiosError(repoError) &&
    repoError.response?.status === 401 &&
    typeof repoError.response?.data === 'object' &&
    repoError.response?.data !== null &&
    'error' in repoError.response.data &&
    typeof (repoError.response.data as Record<string, unknown>).error === 'string' &&
    ((repoError.response.data as Record<string, unknown>).error as string).includes('unlock');

  if (repoLoading) {
    return <LoadingSpinner className="h-full" message="Loading repository..." />;
  }

  if (needsUnlock || showUnlock) {
    return (
      <UnlockModal
        repoName={repoId ?? ''}
        onUnlock={handleUnlock}
        onClose={() => setShowUnlock(false)}
        isUnlocking={unlockRepo.isPending}
        error={unlockError}
      />
    );
  }

  if (repoError) {
    const is404 =
      axios.isAxiosError(repoError) && repoError.response?.status === 404;
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <div className="text-center">
          <h2 className="text-lg font-semibold text-text-primary">
            {is404 ? 'Repository not found' : 'Failed to load repository'}
          </h2>
          <p className="mt-1 text-sm text-text-muted">
            {is404
              ? `The repository "${repoId ?? ''}" does not exist or has been removed.`
              : repoError.message}
          </p>
        </div>
        <div className="flex items-center gap-3">
          <a
            href="/"
            className="rounded border border-border px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:bg-surface-hover"
          >
            Back to repositories
          </a>
          {!is404 && (
            <button
              onClick={() => setShowUnlock(true)}
              className="rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 hover:bg-accent-light"
            >
              Unlock Repository
            </button>
          )}
        </div>
      </div>
    );
  }

  if (!repo) return null;

  const currentBranch = status?.branch ?? repo.head ?? 'main';

  const lastAuthor = log?.commits[0]?.author;

  const handleCommit = (
    message: string,
    authorName: string,
    authorEmail: string,
    options?: { amend?: boolean; sign?: boolean },
  ) => {
    if (!message.trim()) return;
    createCommit.mutate(
      { message, authorName, authorEmail, amend: options?.amend, sign: options?.sign },
      {
        onSuccess: (result) => {
          toast.success(
            `${options?.amend ? 'Amended' : 'Committed'}: ${result.short_id}`,
            {
              label: 'View',
              onClick: () => navigate(`/repo/${repoId}/history`),
            },
          );
          setSelectedChangedFile(null);
        },
        onError: (err: Error) => toast.error(err.message),
      },
    );
  };

  const managementPanel: ReactNode = (
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
          onMerge={handleMerge}
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
          externalShowCreate={showCreateBranch}
          onExternalShowCreateConsumed={() => setShowCreateBranch(false)}
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
  );

  return (
    <div className="flex h-full flex-col">
      <Header
        repoName={repo.name}
        currentBranch={currentBranch}
        branches={branches ?? []}
        syncStatus={syncStatus}
        onCheckout={(name) =>
          checkoutBranch.mutate(name, {
            onSuccess: () => toast.success('Switched to branch'),
            onError: (err: Error) => toast.error(err.message),
          })
        }
        onPush={() => {
          const toastId = toast.progress('Pushing...');
          pushSync.mutate(undefined, {
            onSuccess: () => toast.updateToast(toastId, 'success', 'Pushed successfully'),
            onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
          });
        }}
        onPull={() => {
          const toastId = toast.progress('Pulling...');
          pullSync.mutate(undefined, {
            onSuccess: () => toast.updateToast(toastId, 'success', 'Pulled successfully'),
            onError: (err: Error) => toast.updateToast(toastId, 'error', err.message),
          });
        }}
        isSyncing={pushSync.isPending || pullSync.isPending}
        managementPanel={managementPanel}
      />

      {mergeResult && (
        <div className="border-b border-border p-3">
          <MergePanel
            result={mergeResult}
            onDismiss={() => setMergeResult(null)}
            onAbort={() => {
              resetCommitMutation.mutate(
                { commitId: 'HEAD', mode: 'hard' },
                {
                  onSuccess: () => {
                    toast.success('Merge aborted');
                    setMergeResult(null);
                  },
                  onError: (err: Error) => toast.error(err.message),
                },
              );
            }}
            onStageAndContinue={() => {
              stageFiles.mutate(['.'], {
                onSuccess: () => {
                  toast.success('All files staged. Write a commit message to complete the merge.');
                  setActiveView('status');
                  setMergeResult(null);
                },
                onError: (err: Error) => toast.error(err.message),
              });
            }}
          />
        </div>
      )}

      {browseRef && (
        <div className="flex items-center gap-2 border-b border-border bg-accent/10 px-4 py-2">
          <History size={14} className="flex-shrink-0 text-accent" />
          <span className="text-xs text-text-secondary">
            Browsing files at commit{' '}
            <span className="font-mono font-semibold text-accent">{browseRef.slice(0, 12)}</span>
          </span>
          <button
            onClick={() => {
              setBrowseRef(null);
              setSelectedFile(null);
            }}
            className="ml-auto flex items-center gap-1 rounded bg-accent/20 px-2 py-0.5 text-xs font-medium text-accent transition-colors hover:bg-accent/30"
          >
            <X size={12} />
            Return to HEAD
          </button>
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        {/* Left: File tree / Changes */}
        <div className="flex w-80 flex-shrink-0 flex-col border-r border-border">
          <div className="flex border-b border-border">
            <button
              onClick={() => setActiveView('files')}
              className={`flex flex-1 items-center justify-center gap-1 py-2 text-xs transition-colors ${
                activeView === 'files'
                  ? 'border-b-2 border-accent text-accent'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              <FolderTree size={13} />
              Files
            </button>
            <button
              onClick={() => setActiveView('status')}
              className={`flex flex-1 items-center justify-center gap-1 py-2 text-xs transition-colors ${
                activeView === 'status'
                  ? 'border-b-2 border-accent text-accent'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              <GitPullRequestDraft size={13} />
              Changes
              {status && status.staged.length + status.unstaged.length + status.untracked.length > 0 && (
                <span className="rounded-full bg-accent/20 px-1.5 text-[10px] font-bold text-accent">
                  {status.staged.length + status.unstaged.length + status.untracked.length}
                </span>
              )}
            </button>
          </div>

          <div className="flex-1 overflow-hidden">
            {activeView === 'files' ? (
              <div className="flex h-full flex-col">
                {!browseRef && (
                  <div className="flex items-center gap-1 border-b border-border px-2 py-1">
                    <button
                      onClick={() => setShowNewFileModal(true)}
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                      title="New file"
                      aria-label="New file"
                    >
                      <FilePlus size={12} />
                      New File
                    </button>
                    <button
                      onClick={() => setShowUploadModal(true)}
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                      title="Upload files"
                      aria-label="Upload files"
                    >
                      <Upload size={12} />
                      Upload
                    </button>
                    <button
                      onClick={() => setShowNewFolderModal(true)}
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                      title="New folder"
                      aria-label="New folder"
                    >
                      <FolderPlus size={12} />
                      New Folder
                    </button>
                    <button
                      onClick={() => {
                        void queryClient.invalidateQueries({ queryKey: ['repo', repoId, 'tree'] });
                        void queryClient.invalidateQueries({ queryKey: ['repo', repoId, 'status'] });
                      }}
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                      title="Refresh file tree"
                      aria-label="Refresh file tree"
                    >
                      <RefreshCw size={12} />
                    </button>
                  </div>
                )}
                <div className="flex-1 overflow-hidden">
                  <FileTree
                    repoId={repoId!}
                    selectedPath={selectedFile}
                    onSelectFile={(path) => {
                      setSelectedFile(path);
                      setFileHistoryRequested(false);
                      setHighlightLine(undefined);
                    }}
                    onShowFileHistory={(path) => {
                      setSelectedFile(path);
                      setFileHistoryRequested(true);
                      setActiveView('files');
                    }}
                    statusEntries={[...(status?.staged ?? []), ...(status?.unstaged ?? [])]}
                    browseRef={browseRef ?? undefined}
                  />
                </div>
              </div>
            ) : browseRef ? (
              <div className="flex h-full items-center justify-center text-text-muted">
                <p className="text-xs">Changes view is unavailable when browsing a historical commit</p>
              </div>
            ) : status ? (
              <div className="flex h-full flex-col overflow-hidden">
                <div className="min-h-0 flex-1 overflow-y-auto">
                  <StatusPanel
                    status={status}
                    diff={stagedDiff}
                    unstagedDiff={unstagedDiff}
                    selectedFile={selectedChangedFile}
                    onSelectFile={setSelectedChangedFile}
                    onStage={(paths) => stageFiles.mutate(paths)}
                    onUnstage={(paths) => unstageFiles.mutate(paths)}
                    onRestore={(paths) =>
                      restoreFilesMutation.mutate(paths, {
                        onSuccess: () =>
                          toast.success(`Restored ${paths.length} file${paths.length !== 1 ? 's' : ''}`),
                        onError: (err: Error) => toast.error(err.message),
                      })
                    }
                    onClean={(paths) =>
                      cleanFilesMutation.mutate(
                        { paths },
                        {
                          onSuccess: (result) =>
                            toast.success(`Deleted ${result.deleted.length} file${result.deleted.length !== 1 ? 's' : ''}`),
                          onError: (err: Error) => toast.error(err.message),
                        },
                      )
                    }
                    isStaging={stageFiles.isPending}
                    isUnstaging={unstageFiles.isPending}
                    isRestoring={restoreFilesMutation.isPending}
                    isCleaning={cleanFilesMutation.isPending}
                  />
                </div>
                <CommitForm
                  stagedCount={status.staged.length}
                  onCommit={handleCommit}
                  isCommitting={createCommit.isPending}
                  defaultAuthorName={lastAuthor?.name}
                  defaultAuthorEmail={lastAuthor?.email}
                  lastCommitMessage={log?.commits[0]?.message}
                  repoId={repoId}
                />
              </div>
            ) : (
              <LoadingSpinner className="py-4" size={16} />
            )}
          </div>
        </div>

        {/* Main content: File viewer or Diff viewer */}
        <div className="flex-1 overflow-hidden">
          {activeView === 'status' ? (
            <ChangesDiffView
              stagedDiff={stagedDiff}
              unstagedDiff={unstagedDiff}
              selectedFile={selectedChangedFile}
              isLoading={diffLoading}
            />
          ) : (
            <FileViewer
               key={selectedFile ?? 'empty'}
               repoId={repoId!}
               filePath={selectedFile}
               browseRef={browseRef ?? undefined}
              highlightLine={highlightLine}
              initialShowHistory={fileHistoryRequested}
              onFileDeleted={() => setSelectedFile(null)}
              onNavigateToDir={() => setSelectedFile(null)}
            />
          )}
        </div>
      </div>

      {/* New File Modal */}
      {showNewFileModal && (
        <NewFileModal
          onClose={() => setShowNewFileModal(false)}
          onSave={(path, content) => {
            putBlobMutation.mutate(
              { path, content },
              {
                onSuccess: () => {
                  toast.success(`Created ${path}`);
                  setShowNewFileModal(false);
                  setSelectedFile(path);
                  setActiveView('files');
                },
                onError: (err: Error) => toast.error(`Failed to create file: ${err.message}`),
              },
            );
          }}
          isSaving={putBlobMutation.isPending}
        />
      )}

      {/* Upload Modal */}
      {showUploadModal && (
        <UploadModal
          onClose={() => setShowUploadModal(false)}
          onUpload={(targetPath, files) => {
            const toastId = toast.progress('Uploading files...');
            uploadFilesMutation.mutate(
              { targetPath, files },
              {
                onSuccess: () => {
                  toast.updateToast(toastId, 'success', `Uploaded ${files.length} file${files.length !== 1 ? 's' : ''}`);
                  setShowUploadModal(false);
                },
                onError: (err: Error) => toast.updateToast(toastId, 'error', `Upload failed: ${err.message}`),
              },
            );
          }}
          isUploading={uploadFilesMutation.isPending}
        />
      )}

      {/* New Folder Modal */}
      {showNewFolderModal && (
        <NewFolderModal
          onClose={() => setShowNewFolderModal(false)}
          onCreate={(path) => {
            createDirectoryMutation.mutate(path, {
              onSuccess: () => {
                toast.success(`Created directory ${path}`);
                setShowNewFolderModal(false);
              },
              onError: (err: Error) => toast.error(`Failed to create directory: ${err.message}`),
            });
          }}
          isCreating={createDirectoryMutation.isPending}
        />
      )}
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/*  New File Modal                                                            */
/* -------------------------------------------------------------------------- */

interface NewFileModalProps {
  onClose: () => void;
  onSave: (path: string, content: string) => void;
  isSaving: boolean;
}

function NewFileModal({ onClose, onSave, isSaving }: NewFileModalProps) {
  const [filePath, setFilePath] = useState('');
  const [content, setContent] = useState('');

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (filePath.trim()) {
      onSave(filePath.trim(), content);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-label="Create New File" className="flex w-full max-w-lg flex-col rounded-lg border border-border bg-navy-900 shadow-2xl" style={{ maxHeight: '80vh' }}>
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <FilePlus size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Create New File</h2>
          </div>
          <button onClick={onClose} aria-label="Close" className="rounded p-1 text-text-muted hover:text-text-primary">
            <X size={16} />
          </button>
        </div>
        <form onSubmit={handleSubmit} className="flex min-h-0 flex-1 flex-col p-4">
          <label className="mb-1 block text-xs text-text-muted">File Path</label>
          <input
            value={filePath}
            onChange={(e) => setFilePath(e.target.value)}
            placeholder="src/utils/helpers.ts"
            className="mb-3 w-full rounded border border-border bg-navy-950 px-3 py-2 font-mono text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          <label className="mb-1 block text-xs text-text-muted">Content</label>
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            placeholder="File content..."
            className="min-h-[200px] flex-1 resize-none rounded border border-border bg-navy-950 p-3 font-mono text-[13px] leading-5 text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            spellCheck={false}
          />
          <button
            type="submit"
            disabled={!filePath.trim() || isSaving}
            className="mt-4 w-full rounded bg-accent py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isSaving ? 'Creating...' : 'Create File'}
          </button>
        </form>
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/*  Upload Modal                                                              */
/* -------------------------------------------------------------------------- */

interface UploadModalProps {
  onClose: () => void;
  onUpload: (targetPath: string, files: File[]) => void;
  isUploading: boolean;
}

function UploadModal({ onClose, onUpload, isUploading }: UploadModalProps) {
  const [targetPath, setTargetPath] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  function handleDrop(e: React.DragEvent) {
    e.preventDefault();
    setIsDragging(false);
    const dropped = Array.from(e.dataTransfer.files);
    if (dropped.length > 0) {
      setFiles((prev) => [...prev, ...dropped]);
    }
  }

  function handleFileSelect(e: React.ChangeEvent<HTMLInputElement>) {
    const selected = e.target.files;
    if (selected && selected.length > 0) {
      setFiles((prev) => [...prev, ...Array.from(selected)]);
    }
    // Reset so the same file can be selected again
    e.target.value = '';
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (files.length > 0) {
      onUpload(targetPath.trim() || '.', files);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-label="Upload Files" className="w-full max-w-md rounded-lg border border-border bg-navy-900 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Upload size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Upload Files</h2>
          </div>
          <button onClick={onClose} aria-label="Close" className="rounded p-1 text-text-muted hover:text-text-primary">
            <X size={16} />
          </button>
        </div>
        <form onSubmit={handleSubmit} className="p-4">
          <label className="mb-1 block text-xs text-text-muted">Target Directory</label>
          <input
            value={targetPath}
            onChange={(e) => setTargetPath(e.target.value)}
            placeholder="/ (root)"
            className="mb-3 w-full rounded border border-border bg-navy-950 px-3 py-2 font-mono text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
          />
          <label className="mb-1 block text-xs text-text-muted">Files</label>
          <div
            onDragOver={(e) => { e.preventDefault(); setIsDragging(true); }}
            onDragLeave={() => setIsDragging(false)}
            onDrop={handleDrop}
            onClick={() => fileInputRef.current?.click()}
            className={`flex min-h-[120px] cursor-pointer flex-col items-center justify-center rounded border-2 border-dashed transition-colors ${
              isDragging
                ? 'border-accent bg-accent/10'
                : 'border-border bg-navy-950 hover:border-text-muted'
            }`}
          >
            <Upload size={24} className="mb-2 text-text-muted" />
            <p className="text-xs text-text-muted">
              Drop files here or click to browse
            </p>
            <input
              ref={fileInputRef}
              type="file"
              multiple
              onChange={handleFileSelect}
              className="hidden"
            />
          </div>
          {files.length > 0 && (
            <div className="mt-2 max-h-[150px] overflow-y-auto rounded border border-border bg-navy-950 p-2">
              {files.map((file, idx) => (
                <div key={`${file.name}-${idx}`} className="flex items-center justify-between py-0.5">
                  <span className="truncate font-mono text-xs text-text-secondary">{file.name}</span>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setFiles((prev) => prev.filter((_, i) => i !== idx));
                    }}
                    className="ml-2 flex-shrink-0 rounded p-0.5 text-text-muted hover:text-status-deleted"
                    aria-label={`Remove ${file.name}`}
                  >
                    <X size={12} />
                  </button>
                </div>
              ))}
            </div>
          )}
          <button
            type="submit"
            disabled={files.length === 0 || isUploading}
            className="mt-4 w-full rounded bg-accent py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isUploading ? 'Uploading...' : `Upload ${files.length} File${files.length !== 1 ? 's' : ''}`}
          </button>
        </form>
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/*  New Folder Modal                                                          */
/* -------------------------------------------------------------------------- */

interface NewFolderModalProps {
  onClose: () => void;
  onCreate: (path: string) => void;
  isCreating: boolean;
}

function NewFolderModal({ onClose, onCreate, isCreating }: NewFolderModalProps) {
  const [dirPath, setDirPath] = useState('');

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (dirPath.trim()) {
      onCreate(dirPath.trim());
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-label="Create Directory" className="w-full max-w-sm rounded-lg border border-border bg-navy-900 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <FolderPlus size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Create Directory</h2>
          </div>
          <button onClick={onClose} aria-label="Close" className="rounded p-1 text-text-muted hover:text-text-primary">
            <X size={16} />
          </button>
        </div>
        <form onSubmit={handleSubmit} className="p-4">
          <label className="mb-1 block text-xs text-text-muted">Directory Path</label>
          <input
            value={dirPath}
            onChange={(e) => setDirPath(e.target.value)}
            placeholder="src/components"
            className="w-full rounded border border-border bg-navy-950 px-3 py-2 font-mono text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          <button
            type="submit"
            disabled={!dirPath.trim() || isCreating}
            className="mt-4 w-full rounded bg-accent py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isCreating ? 'Creating...' : 'Create Directory'}
          </button>
        </form>
      </div>
    </div>
  );
}

export default RepoPage;
