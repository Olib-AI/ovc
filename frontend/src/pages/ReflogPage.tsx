import { useState, useCallback, useMemo } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { useQuery } from '@tanstack/react-query';
import { getReflog } from '../api/client.ts';
import { useResetCommit } from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import {
  RotateCcw,
  ArrowRight,
  Clock,
  AlertTriangle,
  GitCommitVertical,
  GitBranch,
  GitMerge,
  Rewind,
  Shuffle,
  Search,
  X,
  ChevronsRight,
} from 'lucide-react';
import type { ReflogEntry, ResetMode } from '../api/types.ts';

function formatRelativeTime(unixTimestamp: number): string {
  const now = Date.now();
  const diffMs = now - unixTimestamp * 1000;
  const diffSec = Math.floor(diffMs / 1000);

  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin} minute${diffMin !== 1 ? 's' : ''} ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr} hour${diffHr !== 1 ? 's' : ''} ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays < 30) return `${diffDays} day${diffDays !== 1 ? 's' : ''} ago`;
  const diffMonths = Math.floor(diffDays / 30);
  if (diffMonths < 12) return `${diffMonths} month${diffMonths !== 1 ? 's' : ''} ago`;
  const diffYears = Math.floor(diffMonths / 12);
  return `${diffYears} year${diffYears !== 1 ? 's' : ''} ago`;
}

// ── Operation type detection ──────────────────────────────────────────────────

type ReflogOpType = 'commit' | 'checkout' | 'merge' | 'rebase' | 'reset' | 'cherry-pick' | 'revert' | 'other';

interface OpStyle {
  icon: typeof GitCommitVertical;
  color: string;
  label: string;
}

const OP_STYLES: Record<ReflogOpType, OpStyle> = {
  commit:       { icon: GitCommitVertical, color: 'text-green-400',   label: 'commit'      },
  checkout:     { icon: GitBranch,         color: 'text-accent',      label: 'checkout'    },
  merge:        { icon: GitMerge,          color: 'text-purple-400',  label: 'merge'       },
  rebase:       { icon: ChevronsRight,     color: 'text-blue-400',    label: 'rebase'      },
  reset:        { icon: Rewind,            color: 'text-amber-400',   label: 'reset'       },
  'cherry-pick':{ icon: GitCommitVertical, color: 'text-pink-400',    label: 'cherry-pick' },
  revert:       { icon: RotateCcw,         color: 'text-orange-400',  label: 'revert'      },
  other:        { icon: Shuffle,           color: 'text-text-muted',  label: 'other'       },
};

function detectOpType(message: string): ReflogOpType {
  const m = message.toLowerCase();
  if (m.startsWith('commit'))      return 'commit';
  if (m.startsWith('checkout'))    return 'checkout';
  if (m.startsWith('merge'))       return 'merge';
  if (m.startsWith('rebase'))      return 'rebase';
  if (m.startsWith('reset'))       return 'reset';
  if (m.startsWith('cherry-pick')) return 'cherry-pick';
  if (m.startsWith('revert'))      return 'revert';
  return 'other';
}

function formatAbsoluteTime(unixTimestamp: number): string {
  return new Date(unixTimestamp * 1000).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

interface ResetConfirmState {
  commitId: string;
  mode: ResetMode;
  shortHash: string;
}

function ReflogPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Reflog \u2014 OVC`);
  const navigate = useNavigate();
  const toast = useToast();
  const resetCommit = useResetCommit(repoId ?? '');
  const [resetConfirm, setResetConfirm] = useState<ResetConfirmState | null>(null);
  const [resetMode, setResetMode] = useState<ResetMode>('mixed');
  const [searchQuery, setSearchQuery] = useState('');

  const { data: entries, isLoading, error } = useQuery({
    queryKey: ['repo', repoId, 'reflog'],
    queryFn: () => getReflog(repoId!),
    enabled: !!repoId,
  });

  const handleClickCommitHash = useCallback(
    (commitHash: string) => {
      navigate(`/repo/${repoId}/history?commit=${commitHash}`);
    },
    [navigate, repoId],
  );

  const handleResetConfirm = useCallback(() => {
    if (!resetConfirm) return;
    const toastId = toast.progress(`Resetting (${resetConfirm.mode})...`);
    resetCommit.mutate(
      { commitId: resetConfirm.commitId, mode: resetConfirm.mode },
      {
        onSuccess: (result) => {
          toast.updateToast(toastId, 'success', `Reset (${result.mode}) to ${result.commit_id.slice(0, 8)}`);
          setResetConfirm(null);
        },
        onError: (err: Error) => {
          toast.updateToast(toastId, 'error', err.message);
        },
      },
    );
  }, [resetConfirm, resetCommit, toast]);

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading reflog..." />;
  }

  if (error) {
    return (
      <div className="p-8 text-sm text-status-deleted">
        Failed to load reflog: {(error as Error).message}
      </div>
    );
  }

  const entryList = entries ?? [];

  return (
    <ReflogPageContent
      entryList={entryList}
      searchQuery={searchQuery}
      setSearchQuery={setSearchQuery}
      resetConfirm={resetConfirm}
      setResetConfirm={setResetConfirm}
      resetMode={resetMode}
      setResetMode={setResetMode}
      handleClickCommitHash={handleClickCommitHash}
      handleResetConfirm={handleResetConfirm}
      resetCommit={resetCommit}
    />
  );
}

interface ReflogPageContentProps {
  entryList: ReflogEntry[];
  searchQuery: string;
  setSearchQuery: (q: string) => void;
  resetConfirm: ResetConfirmState | null;
  setResetConfirm: (v: ResetConfirmState | null) => void;
  resetMode: ResetMode;
  setResetMode: (m: ResetMode) => void;
  handleClickCommitHash: (hash: string) => void;
  handleResetConfirm: () => void;
  resetCommit: { isPending: boolean };
}

function ReflogPageContent({
  entryList,
  searchQuery,
  setSearchQuery,
  resetConfirm,
  setResetConfirm,
  resetMode,
  setResetMode,
  handleClickCommitHash,
  handleResetConfirm,
  resetCommit,
}: ReflogPageContentProps) {
  const filteredEntries = useMemo(() => {
    if (!searchQuery.trim()) return entryList;
    const q = searchQuery.toLowerCase();
    return entryList.filter(
      (e) =>
        e.message.toLowerCase().includes(q) ||
        e.ref_name.toLowerCase().includes(q) ||
        e.identity_name.toLowerCase().includes(q) ||
        e.new_value.startsWith(q) ||
        (e.old_value ?? '').startsWith(q),
    );
  }, [entryList, searchQuery]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <RotateCcw size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Reflog</h1>
        <span className="text-xs text-text-muted">
          {filteredEntries.length}{searchQuery ? ` of ${entryList.length}` : ''} entr{filteredEntries.length !== 1 ? 'ies' : 'y'}
        </span>

        {/* Search filter */}
        <div className="ml-auto flex items-center gap-1.5 rounded border border-border bg-navy-950 px-2 py-1">
          <Search size={12} className="flex-shrink-0 text-text-muted" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Filter entries..."
            className="w-36 bg-transparent text-xs text-text-primary placeholder-text-muted focus:outline-none"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery('')}
              className="flex-shrink-0 text-text-muted hover:text-text-primary"
              aria-label="Clear filter"
            >
              <X size={11} />
            </button>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {filteredEntries.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-text-muted">
            <RotateCcw size={40} className="mb-3 opacity-30" />
            <p className="text-sm">{searchQuery ? 'No matching entries' : 'No reflog entries'}</p>
            {searchQuery && (
              <button onClick={() => setSearchQuery('')} className="mt-2 text-xs text-accent hover:text-accent-light">
                Clear filter
              </button>
            )}
          </div>
        )}

        {filteredEntries.length > 0 && (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-navy-800/50 text-left text-[11px] uppercase tracking-wider text-text-muted">
                <th className="px-4 py-2 font-semibold">Index</th>
                <th className="px-4 py-2 font-semibold">Op</th>
                <th className="px-4 py-2 font-semibold">Ref</th>
                <th className="px-4 py-2 font-semibold">Message</th>
                <th className="px-4 py-2 font-semibold">Refs</th>
                <th className="px-4 py-2 font-semibold">Time</th>
                <th className="px-4 py-2 font-semibold">Author</th>
                <th className="px-4 py-2 font-semibold">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/50">
              {filteredEntries.map((entry, idx) => {
                const opType = detectOpType(entry.message);
                const opStyle = OP_STYLES[opType];
                const OpIcon = opStyle.icon;
                return (
                  <tr
                    key={idx}
                    className="transition-colors hover:bg-surface-hover/50"
                  >
                    <td className="whitespace-nowrap px-4 py-2.5 font-mono text-xs text-accent">
                      HEAD@{'{' + idx + '}'}
                    </td>
                    {/* Operation type icon */}
                    <td className="whitespace-nowrap px-4 py-2.5">
                      <span
                        className={`inline-flex items-center gap-1 rounded border border-current/20 bg-current/5 px-1.5 py-0.5 text-[10px] font-semibold ${opStyle.color}`}
                        title={opStyle.label}
                      >
                        <OpIcon size={10} />
                        {opStyle.label}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5">
                      {entry.ref_name ? (
                        <span
                          className="inline-block max-w-[120px] truncate rounded bg-navy-700/60 border border-border px-1.5 py-0.5 font-mono text-[11px] text-text-secondary"
                          title={entry.ref_name}
                        >
                          {entry.ref_name}
                        </span>
                      ) : (
                        <span className="text-xs text-text-muted/50">--</span>
                      )}
                    </td>
                    <td className="max-w-[300px] truncate px-4 py-2.5 text-xs text-text-primary" title={entry.message}>
                      {entry.message}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5">
                      <div className="flex items-center gap-1.5 font-mono text-xs">
                        <button
                          onClick={() => entry.old_value && handleClickCommitHash(entry.old_value)}
                          disabled={!entry.old_value}
                          className="text-status-modified underline decoration-transparent transition-colors hover:text-accent hover:decoration-accent disabled:cursor-default disabled:no-underline disabled:opacity-50"
                          title={entry.old_value ? `View commit ${entry.old_value.slice(0, 8)}` : ''}
                        >
                          {entry.old_value ? entry.old_value.slice(0, 8) : '00000000'}
                        </button>
                        <ArrowRight size={10} className="text-text-muted" />
                        <button
                          onClick={() => handleClickCommitHash(entry.new_value)}
                          className="text-status-modified underline decoration-transparent transition-colors hover:text-accent hover:decoration-accent"
                          title={`View commit ${entry.new_value.slice(0, 8)}`}
                        >
                          {entry.new_value.slice(0, 8)}
                        </button>
                      </div>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-xs text-text-muted">
                      {entry.timestamp ? (
                        <span
                          className="flex items-center gap-1"
                          title={formatAbsoluteTime(entry.timestamp)}
                        >
                          <Clock size={10} className="flex-shrink-0" />
                          <span>{formatRelativeTime(entry.timestamp)}</span>
                        </span>
                      ) : (
                        <span className="text-text-muted/50">--</span>
                      )}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-xs text-text-muted">
                      {entry.identity_name}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2.5">
                      <button
                        onClick={() =>
                          setResetConfirm({
                            commitId: entry.new_value,
                            mode: resetMode,
                            shortHash: entry.new_value.slice(0, 8),
                          })
                        }
                        className="rounded px-2 py-1 text-[11px] text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                        title="Reset HEAD to this point"
                      >
                        <RotateCcw size={11} className="inline-block" />
                        {' '}Reset
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Reset confirmation dialog */}
      {resetConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-96 rounded-lg border border-border bg-navy-800 p-4 shadow-xl">
            <div className="mb-3 flex items-center gap-2">
              <AlertTriangle size={16} className="text-status-deleted" />
              <h3 className="text-sm font-semibold text-text-primary">Reset HEAD</h3>
            </div>
            <p className="mb-3 text-xs text-text-secondary">
              Reset HEAD to commit{' '}
              <span className="font-mono text-accent">{resetConfirm.shortHash}</span>
            </p>

            <div className="mb-4 space-y-1.5">
              <label className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                Mode
              </label>
              <div className="flex gap-1.5">
                {(['soft', 'mixed', 'hard'] as const).map((mode) => (
                  <button
                    key={mode}
                    onClick={() => {
                      setResetMode(mode);
                      setResetConfirm({ ...resetConfirm, mode });
                    }}
                    className={`flex-1 rounded px-2 py-1.5 text-xs font-medium transition-colors ${
                      resetConfirm.mode === mode
                        ? mode === 'hard'
                          ? 'bg-status-deleted/20 text-status-deleted ring-1 ring-status-deleted/40'
                          : 'bg-accent/15 text-accent ring-1 ring-accent/40'
                        : 'bg-surface text-text-muted hover:text-text-secondary'
                    }`}
                  >
                    {mode}
                  </button>
                ))}
              </div>
              <p className="text-[11px] text-text-muted">
                {resetConfirm.mode === 'soft' && 'Keeps all changes staged. Safe.'}
                {resetConfirm.mode === 'mixed' && 'Unstages changes but keeps them in working directory.'}
                {resetConfirm.mode === 'hard' && 'Discards ALL uncommitted changes. This is destructive and cannot be undone.'}
              </p>
            </div>

            <div className="flex justify-end gap-2">
              <button
                onClick={() => setResetConfirm(null)}
                className="rounded px-3 py-1.5 text-xs text-text-muted hover:text-text-primary"
              >
                Cancel
              </button>
              <button
                onClick={handleResetConfirm}
                disabled={resetCommit.isPending}
                className={`rounded px-3 py-1.5 text-xs font-medium disabled:opacity-50 ${
                  resetConfirm.mode === 'hard'
                    ? 'bg-status-deleted text-white hover:opacity-90'
                    : 'bg-accent text-navy-950 hover:bg-accent-light'
                }`}
              >
                {resetCommit.isPending ? 'Resetting...' : `Reset (${resetConfirm.mode})`}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default ReflogPage;
