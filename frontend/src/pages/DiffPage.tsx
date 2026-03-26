import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { useDiff, useCompare, useBranches, useTags } from '../hooks/useRepo.ts';
import { useQueryClient } from '@tanstack/react-query';
import DiffViewer from '../components/DiffViewer.tsx';
import { DiffSkeleton } from '../components/Skeleton.tsx';
import { Diff, GitCompareArrows, RefreshCw } from 'lucide-react';
import type { DiffResponse } from '../api/types.ts';

type PageMode = 'working' | 'compare';

function DiffPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Diff \u2014 OVC`);
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<PageMode>('working');
  const [staged, setStaged] = useState(true);

  // Compare mode state
  const [baseRef, setBaseRef] = useState('');
  const [headRef, setHeadRef] = useState('');

  const { data: diff, isLoading: isLoadingDiff, error: diffError } = useDiff(repoId, staged);
  const { data: compareDiff, isLoading: isLoadingCompare, error: compareError } = useCompare(
    repoId,
    baseRef,
    headRef,
  );
  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);

  // Build ref suggestions (branches + tags) for datalist
  const refOptions = [
    ...(branches?.map((b) => b.name) ?? []),
    ...(tags?.map((t) => t.name) ?? []),
  ];

  const isLoading = mode === 'working' ? isLoadingDiff : isLoadingCompare;
  const error = mode === 'working' ? diffError : compareError;

  // DiffViewer expects DiffResponse; compareCommits returns CompareResponse which
  // has the same files/stats shape — cast through a structural check.
  const activeDiff: DiffResponse | undefined =
    mode === 'working'
      ? diff
      : compareDiff
        ? { files: compareDiff.files, stats: compareDiff.stats }
        : undefined;

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="w-full max-w-sm text-center">
          <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-status-deleted/10">
            <Diff size={20} className="text-status-deleted" />
          </div>
          <p className="mb-1 text-sm font-semibold text-text-primary">Failed to load diff</p>
          <p className="mb-4 text-xs text-text-secondary">{error.message}</p>
          <button
            onClick={() =>
              void queryClient.invalidateQueries({
                queryKey: ['repo', repoId, mode === 'working' ? 'diff' : 'compare'],
              })
            }
            className="mx-auto flex items-center gap-1.5 rounded bg-accent px-4 py-2 text-sm font-medium text-navy-950 transition-colors hover:bg-accent-light"
          >
            <RefreshCw size={14} />
            Retry
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Diff size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">
          {mode === 'working'
            ? staged ? 'Staged Changes' : 'Unstaged Changes'
            : 'Compare Refs'}
        </h1>

        {/* Mode tabs */}
        <div className="ml-auto flex rounded border border-border">
          <button
            onClick={() => setMode('working')}
            className={`flex items-center gap-1.5 px-3 py-1 text-xs font-medium transition-colors ${
              mode === 'working'
                ? 'bg-accent/15 text-accent'
                : 'text-text-muted hover:text-text-secondary'
            }`}
          >
            <Diff size={12} />
            Working Tree
          </button>
          <button
            onClick={() => setMode('compare')}
            className={`flex items-center gap-1.5 px-3 py-1 text-xs font-medium transition-colors ${
              mode === 'compare'
                ? 'bg-accent/15 text-accent'
                : 'text-text-muted hover:text-text-secondary'
            }`}
          >
            <GitCompareArrows size={12} />
            Compare
          </button>
        </div>
      </div>

      {/* Working tree sub-tabs */}
      {mode === 'working' && (
        <div className="flex items-center gap-2 border-b border-border bg-navy-900/50 px-4 py-1.5">
          <div className="flex rounded border border-border">
            <button
              onClick={() => setStaged(true)}
              className={`px-3 py-1 text-xs font-medium transition-colors ${
                staged
                  ? 'bg-accent/15 text-accent'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              Staged
            </button>
            <button
              onClick={() => setStaged(false)}
              className={`px-3 py-1 text-xs font-medium transition-colors ${
                !staged
                  ? 'bg-accent/15 text-accent'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              Unstaged
            </button>
          </div>
        </div>
      )}

      {/* Compare ref inputs */}
      {mode === 'compare' && (
        <div className="flex items-center gap-2 border-b border-border bg-navy-900/50 px-4 py-2">
          <datalist id="ref-options">
            {refOptions.map((r) => (
              <option key={r} value={r} />
            ))}
          </datalist>
          <div className="flex flex-1 items-center gap-2">
            <label className="text-xs text-text-muted">Base</label>
            <input
              value={baseRef}
              onChange={(e) => setBaseRef(e.target.value)}
              placeholder="branch, tag, or commit hash"
              list="ref-options"
              className="min-w-0 flex-1 rounded border border-border bg-navy-950 px-2 py-1 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          </div>
          <span className="text-text-muted">
            <GitCompareArrows size={14} />
          </span>
          <div className="flex flex-1 items-center gap-2">
            <label className="text-xs text-text-muted">Head</label>
            <input
              value={headRef}
              onChange={(e) => setHeadRef(e.target.value)}
              placeholder="branch, tag, or commit hash"
              list="ref-options"
              className="min-w-0 flex-1 rounded border border-border bg-navy-950 px-2 py-1 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          </div>
        </div>
      )}

      <div className="flex-1 overflow-x-auto overflow-y-hidden">
        {mode === 'compare' && !baseRef && !headRef && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-text-muted">
            <GitCompareArrows size={28} />
            <p className="text-sm">Enter two refs above to compare them</p>
          </div>
        )}
        {(mode !== 'compare' || baseRef || headRef) && (
          <>
            {isLoading ? (
              <DiffSkeleton />
            ) : activeDiff && activeDiff.files.length > 0 ? (
              <DiffViewer diff={activeDiff} repoId={repoId} />
            ) : !isLoading && (
              <div className="flex h-full items-center justify-center text-text-muted">
                <p className="text-sm">
                  {mode === 'working'
                    ? `No ${staged ? 'staged' : 'unstaged'} changes`
                    : 'No differences between the selected refs'}
                </p>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export default DiffPage;
