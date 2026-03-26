import { useState, useRef, useEffect } from 'react';
import {
  GitPullRequest,
  GitMerge,
  GitCommitVertical,
  AlertTriangle,
  Check,
  ArrowRight,
  ChevronDown,
  ChevronRight,
  Loader2,
} from 'lucide-react';
import type { PullRequestView as PullRequestViewData } from '../api/types.ts';
import type { MergeStrategy } from '../api/client.ts';
import DiffViewer from './DiffViewer.tsx';

interface PullRequestViewProps {
  data: PullRequestViewData;
  repoId: string;
  onMerge: (strategy: MergeStrategy) => void;
  isMerging: boolean;
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

const MERGE_STRATEGIES: { value: MergeStrategy; label: string; description: string }[] = [
  { value: 'merge', label: 'Merge commit', description: 'All commits will be added with a merge commit' },
  { value: 'squash', label: 'Squash and merge', description: 'Squash all commits into a single commit' },
  { value: 'rebase', label: 'Rebase and merge', description: 'Rebase commits onto the target branch' },
];

interface MergeSplitButtonProps {
  mergeable: boolean;
  isMerging: boolean;
  branchName: string;
  baseName: string;
  onMerge: (strategy: MergeStrategy) => void;
}

function MergeSplitButton({ mergeable, isMerging, branchName, baseName, onMerge }: MergeSplitButtonProps) {
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<MergeStrategy>('merge');
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    if (open) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [open]);

  const selectedLabel = MERGE_STRATEGIES.find((s) => s.value === selected)?.label ?? 'Merge';

  return (
    <div className="relative" ref={dropdownRef}>
      <div className="flex">
        <button
          onClick={() => onMerge(selected)}
          disabled={!mergeable || isMerging}
          className="flex items-center gap-1.5 rounded-l-md bg-accent px-3 py-1.5 text-xs font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:cursor-not-allowed disabled:opacity-50"
          title={mergeable ? `${selectedLabel}: ${branchName} into ${baseName}` : 'Cannot merge: conflicts detected'}
        >
          {isMerging ? (
            <Loader2 size={13} className="animate-spin" />
          ) : (
            <GitMerge size={13} />
          )}
          {isMerging ? 'Merging...' : selectedLabel}
        </button>
        <button
          onClick={() => setOpen((v) => !v)}
          disabled={!mergeable || isMerging}
          className="flex items-center rounded-r-md border-l border-navy-950/20 bg-accent px-1.5 py-1.5 text-navy-950 transition-colors hover:bg-accent-light disabled:cursor-not-allowed disabled:opacity-50"
          aria-label="Select merge strategy"
          aria-expanded={open}
        >
          <ChevronDown size={13} />
        </button>
      </div>

      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 w-64 overflow-hidden rounded-md border border-border bg-navy-800 shadow-lg">
          {MERGE_STRATEGIES.map((strategy) => (
            <button
              key={strategy.value}
              onClick={() => {
                setSelected(strategy.value);
                setOpen(false);
              }}
              className={`flex w-full flex-col gap-0.5 px-3 py-2 text-left transition-colors hover:bg-surface-hover ${
                selected === strategy.value ? 'bg-accent/10' : ''
              }`}
            >
              <span className={`text-xs font-semibold ${selected === strategy.value ? 'text-accent' : 'text-text-primary'}`}>
                {strategy.label}
              </span>
              <span className="text-[11px] text-text-muted">{strategy.description}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function PullRequestView({ data, repoId, onMerge, isMerging }: PullRequestViewProps) {
  const [commitsOpen, setCommitsOpen] = useState(true);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="flex-shrink-0 border-b border-border bg-navy-900 px-6 py-4">
        <div className="flex flex-wrap items-start gap-3">
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <GitPullRequest size={18} className="flex-shrink-0 text-accent" />
            <div className="flex min-w-0 items-center gap-1.5 font-mono text-sm">
              <span className="truncate font-semibold text-text-primary">{data.branch}</span>
              <ArrowRight size={14} className="flex-shrink-0 text-text-muted" />
              <span className="text-text-secondary">{data.base}</span>
            </div>
          </div>

          <div className="flex flex-shrink-0 items-center gap-2">
            {/* Ahead/behind badges */}
            <span className="rounded-full border border-border bg-navy-800 px-2.5 py-0.5 text-[11px] text-text-secondary">
              {data.ahead_by} ahead
            </span>
            <span className="rounded-full border border-border bg-navy-800 px-2.5 py-0.5 text-[11px] text-text-secondary">
              {data.behind_by} behind
            </span>

            {/* Merge status badge */}
            {data.mergeable ? (
              <span className="flex items-center gap-1 rounded-full bg-status-added/15 px-2.5 py-0.5 text-[11px] font-medium text-status-added">
                <Check size={11} />
                Ready to merge
              </span>
            ) : (
              <span className="flex items-center gap-1 rounded-full bg-status-deleted/15 px-2.5 py-0.5 text-[11px] font-medium text-status-deleted">
                <AlertTriangle size={11} />
                Has conflicts
              </span>
            )}

            {/* Merge split button */}
            <MergeSplitButton
              mergeable={data.mergeable}
              isMerging={isMerging}
              branchName={data.branch}
              baseName={data.base}
              onMerge={onMerge}
            />
          </div>
        </div>
      </div>

      {/* Body — scrollable */}
      <div className="flex min-h-0 flex-1 flex-col gap-0 overflow-y-auto">
        {/* Conflicts section — shown first if not mergeable */}
        {!data.mergeable && data.conflict_files.length > 0 && (
          <section className="border-b border-border bg-status-deleted/5 px-6 py-4">
            <h2 className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-status-deleted">
              <AlertTriangle size={13} />
              Conflicting Files ({data.conflict_files.length})
            </h2>
            <ul className="space-y-1" role="list">
              {data.conflict_files.map((file) => (
                <li key={file} className="flex items-center gap-2">
                  <AlertTriangle size={12} className="flex-shrink-0 text-status-deleted" />
                  <span className="font-mono text-xs text-text-primary">{file}</span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {/* Commits section */}
        <section className="border-b border-border">
          <button
            onClick={() => setCommitsOpen((v) => !v)}
            className="flex w-full items-center gap-2 bg-navy-800/50 px-6 py-3 text-left transition-colors hover:bg-navy-800"
            aria-expanded={commitsOpen}
          >
            {commitsOpen ? (
              <ChevronDown size={14} className="flex-shrink-0 text-text-muted" />
            ) : (
              <ChevronRight size={14} className="flex-shrink-0 text-text-muted" />
            )}
            <GitCommitVertical size={14} className="flex-shrink-0 text-accent" />
            <span className="text-xs font-semibold text-text-primary">
              Commits ({data.commits.length})
            </span>
            <span className="ml-1 text-xs text-text-muted">unique to {data.branch}</span>
          </button>

          {commitsOpen && (
            <ul className="divide-y divide-border/50 px-6" role="list">
              {data.commits.length === 0 ? (
                <li className="py-4 text-center text-xs text-text-muted">No unique commits</li>
              ) : (
                data.commits.map((commit) => (
                  <li key={commit.id} className="flex items-start gap-3 py-3">
                    <GitCommitVertical size={13} className="mt-0.5 flex-shrink-0 text-text-muted" />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-xs text-text-primary">{commit.message}</p>
                      <p className="mt-0.5 text-[11px] text-text-muted">
                        <span className="font-mono text-accent/80">{commit.short_id}</span>
                        {' · '}
                        {commit.author.name}
                        {' · '}
                        {formatDate(commit.authored_at)}
                      </p>
                    </div>
                  </li>
                ))
              )}
            </ul>
          )}
        </section>

        {/* Files changed / diff section */}
        <section className="min-h-0 flex-1">
          <div className="sticky top-0 z-10 border-b border-border bg-navy-800/50 px-6 py-3">
            <div className="flex items-center gap-3">
              <h2 className="flex items-center gap-2 text-xs font-semibold text-text-primary">
                <span className="uppercase tracking-wider text-text-muted">Files Changed</span>
                <span className="rounded-full bg-accent/15 px-2 py-0.5 text-[11px] font-semibold text-accent">
                  {data.diff.stats.files_changed}
                </span>
              </h2>
              <div className="flex items-center gap-2 ml-auto">
                <span className="text-[11px] font-semibold text-green-400">
                  +{data.diff.stats.additions.toLocaleString()}
                </span>
                <span className="text-[11px] font-semibold text-status-deleted">
                  -{data.diff.stats.deletions.toLocaleString()}
                </span>
                {/* Visual bar showing ratio of additions to deletions */}
                {(data.diff.stats.additions + data.diff.stats.deletions) > 0 && (
                  <div className="h-2 w-20 overflow-hidden rounded-full bg-status-deleted/30" title={`+${data.diff.stats.additions} / -${data.diff.stats.deletions}`}>
                    <div
                      className="h-full bg-green-400 transition-all"
                      style={{
                        width: `${Math.round((data.diff.stats.additions / (data.diff.stats.additions + data.diff.stats.deletions)) * 100)}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            </div>
          </div>
          <DiffViewer diff={data.diff} repoId={repoId} />
        </section>
      </div>
    </div>
  );
}

export default PullRequestView;
