import { X, Play, Copy, Archive, Hash, GitBranch, Clock } from 'lucide-react';
import type { StashEntryInfo } from '../api/types.ts';
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut.ts';
import { useCommitDiff } from '../hooks/useRepo.ts';
import DiffViewer from './DiffViewer.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';

interface StashPreviewProps {
  repoId: string;
  stash: StashEntryInfo;
  onPop: (idx: number) => void;
  onApply: (idx: number) => void;
  onClose: () => void;
}

function StashPreview({ repoId, stash, onPop, onApply, onClose }: StashPreviewProps) {
  useKeyboardShortcut('Escape', onClose);

  const { data: diff, isLoading: diffLoading } = useCommitDiff(repoId, stash.commit_id);

  const timestamp = new Date(stash.timestamp * 1000);
  const formattedDate = timestamp.toLocaleString();

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="fixed inset-0 bg-navy-950/70" onClick={onClose} />
      <div className="relative z-10 flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-navy-800 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Archive size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">
              stash@{'{' + stash.index + '}'}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            aria-label="Close stash preview"
          >
            <X size={16} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="space-y-3 px-4 py-4">
            <div className="rounded-lg border border-border bg-navy-900 p-3 text-sm text-text-primary">
              {stash.message}
            </div>

            <div className="space-y-2 text-xs">
              <div className="flex items-center gap-2">
                <Hash size={12} className="flex-shrink-0 text-text-muted" />
                <span className="text-text-muted">Commit ID</span>
                <span className="ml-auto font-mono text-text-secondary">
                  {stash.commit_id.slice(0, 12)}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <GitBranch size={12} className="flex-shrink-0 text-text-muted" />
                <span className="text-text-muted">Base commit</span>
                <span className="ml-auto font-mono text-text-secondary">
                  {stash.base_commit_id.slice(0, 12)}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <Clock size={12} className="flex-shrink-0 text-text-muted" />
                <span className="text-text-muted">Timestamp</span>
                <span className="ml-auto text-text-secondary">{formattedDate}</span>
              </div>
            </div>
          </div>

          {/* Stash diff */}
          <div className="border-t border-border">
            {diffLoading && <LoadingSpinner className="py-6" size={16} message="Loading diff..." />}
            {diff && <DiffViewer diff={diff} />}
            {!diffLoading && !diff && (
              <p className="px-4 py-4 text-xs text-text-muted">No diff available</p>
            )}
          </div>
        </div>

        <div className="flex gap-2 border-t border-border px-4 py-3">
          <button
            onClick={() => {
              onApply(stash.index);
              onClose();
            }}
            className="flex flex-1 items-center justify-center gap-1.5 rounded border border-border bg-surface py-2 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
          >
            <Copy size={13} />
            Apply
          </button>
          <button
            onClick={() => {
              onPop(stash.index);
              onClose();
            }}
            className="flex flex-1 items-center justify-center gap-1.5 rounded bg-accent py-2 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light"
          >
            <Play size={13} />
            Pop
          </button>
        </div>
      </div>
    </div>
  );
}

export default StashPreview;
