import { useState, useMemo, useCallback, useEffect, useRef } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import {
  useCommitLog,
  useBranches,
  useTags,
  useCherryPick,
  useRevertCommit,
  useCreateBranch,
  useCreateTag,
  useNotes,
  useResetCommit,
  useRepo,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getCommit } from '../api/client.ts';
import CommitGraph from '../components/CommitGraph.tsx';
import CommitDetail from '../components/CommitDetail.tsx';
import ContributorStats from '../components/ContributorStats.tsx';
import { CommitListSkeleton } from '../components/Skeleton.tsx';
import { History, Search, Loader2, AlertTriangle, ArrowLeft, RefreshCw } from 'lucide-react';
import axios from 'axios';
import type { ResetMode } from '../api/types.ts';

function HistoryPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 History \u2014 OVC`);
  const navigate = useNavigate();
  const [urlSearchParams, setUrlSearchParams] = useSearchParams();
  const [selectedCommitId, setSelectedCommitId] = useState<string | null>(null);
  // Cursor-based pagination: accumulate pages of commits client-side.
  // Each page uses the last commit's ID as the `after` cursor so the server
  // returns only commits strictly older than that point — no re-fetching of
  // already-seen commits on each "Load more" click.
  const PAGE_SIZE = 50;
  const [allCommits, setAllCommits] = useState<import('../api/types.ts').CommitInfo[]>([]);
  const [afterCursor, setAfterCursor] = useState<string | undefined>(undefined);
  const [hasMore, setHasMore] = useState(true);

  // Consume ?commit= search param (e.g. from ReflogPage navigation)
  useEffect(() => {
    const commitParam = urlSearchParams.get('commit');
    if (commitParam) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setSelectedCommitId(commitParam);
      setUrlSearchParams((prev) => {
        const next = new URLSearchParams(prev);
        next.delete('commit');
        return next;
      }, { replace: true });
    }
  }, [urlSearchParams, setUrlSearchParams]);
  const [searchQuery, setSearchQuery] = useState('');
  const [promptState, setPromptState] = useState<{
    kind: 'branch' | 'tag';
    commitId: string;
  } | null>(null);
  const [promptValue, setPromptValue] = useState('');
  const [cherryPickConfirm, setCherryPickConfirm] = useState<string | null>(null);
  const toast = useToast();

  const { data: log, isLoading, isFetching, error: logError } = useCommitLog(repoId, PAGE_SIZE, afterCursor);

  // Append newly fetched page into the accumulated list.
  // The effect only fires when `log` identity changes (i.e. new page arrived).
  useEffect(() => {
    if (!log) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setAllCommits((prev) => {
      // De-duplicate by id in case of concurrent fetches or stale re-renders.
      const existingIds = new Set(prev.map((c) => c.id));
      const fresh = log.commits.filter((c) => !existingIds.has(c.id));
      return fresh.length > 0 ? [...prev, ...fresh] : prev;
    });
    // If the page came back with fewer commits than PAGE_SIZE, there are no more.
    setHasMore(log.commits.length >= PAGE_SIZE);
  }, [log]);
  const { data: repoInfo } = useRepo(repoId);
  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);

  const { data: notes } = useNotes(repoId);

  const cherryPick = useCherryPick(repoId ?? '');
  const revertCommit = useRevertCommit(repoId ?? '');
  const createBranch = useCreateBranch(repoId ?? '');
  const createTag = useCreateTag(repoId ?? '');
  const resetCommitMutation = useResetCommit(repoId ?? '');

  const { data: selectedCommit } = useQuery({
    queryKey: ['repo', repoId, 'commit', selectedCommitId],
    queryFn: () => getCommit(repoId!, selectedCommitId!),
    enabled: !!repoId && !!selectedCommitId,
    gcTime: 30_000, // per-commit data accumulates — GC after 30s unmounted
  });

  // Evict previous commit's diff from cache when selection changes.
  // Commit diffs are heavy and accumulate unboundedly while the user
  // clicks through history — only the current diff needs to stay cached.
  const queryClient = useQueryClient();
  const prevCommitRef = useRef<string | null>(null);
  useEffect(() => {
    if (prevCommitRef.current && prevCommitRef.current !== selectedCommitId) {
      queryClient.removeQueries({
        queryKey: ['repo', repoId, 'commitDiff', prevCommitRef.current],
      });
    }
    prevCommitRef.current = selectedCommitId ?? null;
  }, [selectedCommitId, repoId, queryClient]);

  const handleCherryPickConfirm = useCallback(
    (commitId: string) => {
      setCherryPickConfirm(commitId);
    },
    [],
  );

  const handleCherryPickExecute = useCallback(
    (commitId: string) => {
      setCherryPickConfirm(null);
      cherryPick.mutate(commitId, {
        onSuccess: () => toast.success('Cherry-pick successful'),
        onError: (err: Error) => toast.error(err.message),
      });
    },
    [cherryPick, toast],
  );

  const handleRevert = useCallback(
    (commitId: string) => {
      revertCommit.mutate(commitId, {
        onSuccess: (result) =>
          toast.success(`Commit reverted. New commit: ${result.new_commit_id.slice(0, 8)}`),
        onError: (err: Error) => toast.error(err.message),
      });
    },
    [revertCommit, toast],
  );

  const handleCreateBranchFromCommit = useCallback(
    (name: string, commitId: string) => {
      createBranch.mutate(
        { name, startPoint: commitId },
        {
          onSuccess: () => toast.success(`Branch "${name}" created`),
          onError: (err: Error) => toast.error(err.message),
        },
      );
    },
    [createBranch, toast],
  );

  const handleCreateTagAtCommit = useCallback(
    (name: string, commitId: string, message?: string) => {
      createTag.mutate(
        { name, commitId, message },
        {
          onSuccess: () => toast.success(`Tag "${name}" created`),
          onError: (err: Error) => toast.error(err.message),
        },
      );
    },
    [createTag, toast],
  );

  const handleResetToCommit = useCallback(
    (commitId: string, mode: ResetMode) => {
      const toastId = toast.progress(`Resetting (${mode})...`);
      resetCommitMutation.mutate(
        { commitId, mode },
        {
          onSuccess: (result) => {
            toast.updateToast(toastId, 'success', `Reset (${result.mode}) to ${result.commit_id.slice(0, 8)}`);
          },
          onError: (err: Error) => {
            toast.updateToast(toastId, 'error', err.message);
          },
        },
      );
    },
    [resetCommitMutation, toast],
  );

  const handleCopyHash = useCallback(
    (commitId: string) => {
      void navigator.clipboard.writeText(commitId);
      toast.success('Commit hash copied');
    },
    [toast],
  );

  const handleBrowseFiles = useCallback(
    (commitId: string) => {
      navigate(`/repo/${repoId}?ref=${commitId}`);
    },
    [navigate, repoId],
  );

  // Context menu handlers that open a prompt modal
  const handleContextCreateBranch = useCallback(
    (commitId: string) => {
      setPromptState({ kind: 'branch', commitId });
      setPromptValue('');
    },
    [],
  );

  const handleContextCreateTag = useCallback(
    (commitId: string) => {
      setPromptState({ kind: 'tag', commitId });
      setPromptValue('');
    },
    [],
  );

  const handlePromptSubmit = useCallback(() => {
    const value = promptValue.trim();
    if (!value || !promptState) return;
    if (promptState.kind === 'branch') {
      handleCreateBranchFromCommit(value, promptState.commitId);
    } else {
      handleCreateTagAtCommit(value, promptState.commitId);
    }
    setPromptState(null);
    setPromptValue('');
  }, [promptValue, promptState, handleCreateBranchFromCommit, handleCreateTagAtCommit]);

  const notedCommitIds = useMemo(() => {
    if (!notes) return new Set<string>();
    return new Set(notes.map((n) => n.commit_id));
  }, [notes]);

  const filteredCommits = useMemo(() => {
    if (!searchQuery.trim()) return allCommits;
    const q = searchQuery.toLowerCase();
    return allCommits.filter(
      (c) =>
        c.message.toLowerCase().includes(q) ||
        c.author.name.toLowerCase().includes(q) ||
        c.author.email.toLowerCase().includes(q) ||
        c.id.toLowerCase().startsWith(q) ||
        c.short_id.toLowerCase().startsWith(q),
    );
  }, [allCommits, searchQuery]);

  // Show loading for initial fetch AND when repoId changes (SPA navigation).
  // `allCommits` starts as [] so we guard on isLoading for the first page.
  if (isLoading && allCommits.length === 0) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
          <History size={16} className="text-accent" />
          <h1 className="text-sm font-semibold text-text-primary">Commit History</h1>
        </div>
        <CommitListSkeleton rows={16} />
      </div>
    );
  }

  if (logError) {
    const apiMessage = axios.isAxiosError(logError)
      ? ((logError.response?.data as { error?: { message?: string } } | undefined)?.error?.message ?? logError.message)
      : logError.message;
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="w-full max-w-md rounded-lg border border-status-deleted/30 bg-status-deleted/5 p-6 shadow-lg">
          <div className="flex items-start gap-3">
            <AlertTriangle size={20} className="mt-0.5 flex-shrink-0 text-status-deleted" />
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold text-text-primary">Failed to load history</p>
              <p className="mt-1 text-xs text-text-secondary">{apiMessage}</p>
              <div className="mt-4 flex items-center gap-3">
                <button
                  onClick={() => {
                    void queryClient.invalidateQueries({ queryKey: ['repo', repoId, 'log'] });
                  }}
                  className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 hover:bg-accent-light"
                >
                  <RefreshCw size={12} />
                  Retry
                </button>
                <button
                  onClick={() => navigate(-1)}
                  className="flex items-center gap-1.5 rounded px-3 py-1.5 text-xs text-text-muted hover:text-text-primary"
                >
                  <ArrowLeft size={12} />
                  Back
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <History size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Commit History</h1>
        {allCommits.length > 0 && (
          <span className="text-xs text-text-muted">
            {allCommits.length} commit{allCommits.length !== 1 ? 's' : ''}
            {hasMore ? '+' : ''}
          </span>
        )}
      </div>

      <div className="flex flex-1 overflow-hidden">
        <div className="flex w-full flex-shrink-0 flex-col border-r border-border sm:w-[560px] sm:flex-shrink-0">
          {/* Contributor stats — collapsible panel */}
          {repoId && <ContributorStats repoId={repoId} />}

          {/* Search input */}
          <div className="border-b border-border px-3 py-2">
            <div className="flex items-center gap-2 rounded border border-border bg-navy-950 px-2 py-1">
              <Search size={13} className="flex-shrink-0 text-text-muted" />
              <input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Filter by message, author..."
                className="flex-1 bg-transparent text-xs text-text-primary placeholder-text-muted focus:outline-none"
              />
            </div>
          </div>

          <div className="flex-1 overflow-hidden">
            <CommitGraph
              commits={filteredCommits}
              branches={branches ?? []}
              tags={tags ?? []}
              selectedCommitId={selectedCommitId}
              onSelectCommit={setSelectedCommitId}
              onCopyHash={handleCopyHash}
              onCreateBranch={handleContextCreateBranch}
              onCherryPick={handleCherryPickConfirm}
              onCreateTag={handleContextCreateTag}
              onRevert={handleRevert}
              onReset={handleResetToCommit}
              notedCommitIds={notedCommitIds}
              isLoading={isLoading && allCommits.length === 0}
            />
          </div>

          {hasMore && allCommits.length > 0 && (
            <div className="border-t border-border p-2 text-center">
              <button
                onClick={() => {
                  const lastCommit = allCommits[allCommits.length - 1];
                  if (lastCommit) setAfterCursor(lastCommit.id);
                }}
                disabled={isFetching}
                className="mx-auto flex items-center gap-1.5 rounded px-3 py-1.5 text-xs text-accent transition-colors hover:bg-accent/10 disabled:opacity-60"
              >
                {isFetching && <Loader2 size={12} className="animate-spin" />}
                {isFetching ? 'Loading...' : 'Load more'}
              </button>
            </div>
          )}
        </div>

        <div className="hidden flex-1 overflow-hidden sm:block">
          {selectedCommit && repoId ? (
            <CommitDetail
              repoId={repoId}
              repoName={repoInfo?.name}
              commit={selectedCommit}
              onCherryPick={handleCherryPickConfirm}
              onRevert={handleRevert}
              onCreateBranch={handleCreateBranchFromCommit}
              onCreateTag={handleCreateTagAtCommit}
              onBrowseFiles={handleBrowseFiles}
              onReset={handleResetToCommit}
            />
          ) : (
            <div className="flex h-full items-center justify-center text-text-muted">
              <p className="text-sm">Select a commit to view details</p>
            </div>
          )}
        </div>
      </div>

      {/* Cherry-pick confirmation modal */}
      {cherryPickConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-80 rounded-lg border border-border bg-navy-800 p-4 shadow-xl">
            <h3 className="mb-3 text-sm font-semibold text-text-primary">Cherry-pick Commit</h3>
            <p className="mb-3 text-xs text-text-secondary">
              Cherry-pick commit{' '}
              <span className="font-mono text-accent/70">{cherryPickConfirm.slice(0, 12)}</span>{' '}
              onto the current branch? This will create a new commit.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setCherryPickConfirm(null)}
                className="rounded px-3 py-1 text-xs text-text-muted hover:text-text-primary"
              >
                Cancel
              </button>
              <button
                onClick={() => handleCherryPickExecute(cherryPickConfirm)}
                className="rounded bg-accent px-3 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light"
              >
                Cherry-pick
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Prompt modal for context menu branch/tag creation */}
      {promptState && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-80 rounded-lg border border-border bg-navy-800 p-4 shadow-xl">
            <h3 className="mb-3 text-sm font-semibold text-text-primary">
              {promptState.kind === 'branch' ? 'Create Branch' : 'Create Tag'}
            </h3>
            <p className="mb-2 text-xs text-text-muted">
              At commit <span className="font-mono text-accent/70">{promptState.commitId.slice(0, 12)}</span>
            </p>
            <input
              value={promptValue}
              onChange={(e) => setPromptValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handlePromptSubmit()}
              placeholder={promptState.kind === 'branch' ? 'Branch name...' : 'Tag name...'}
              className="mb-3 w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => { setPromptState(null); setPromptValue(''); }}
                className="rounded px-3 py-1 text-xs text-text-muted hover:text-text-primary"
              >
                Cancel
              </button>
              <button
                onClick={handlePromptSubmit}
                disabled={!promptValue.trim()}
                className="rounded bg-accent px-3 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default HistoryPage;
