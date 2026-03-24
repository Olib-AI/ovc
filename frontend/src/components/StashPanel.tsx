import { useState } from 'react';
import { Archive, Plus, Play, Copy, Trash2, X, Eye, Eraser, Clock } from 'lucide-react';
import type { StashEntryInfo } from '../api/types.ts';
import StashPreview from './StashPreview.tsx';

function formatStashAge(timestamp: number): string {
  const diffMs = Date.now() - timestamp * 1000;
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 1) return 'just now';
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays < 30) return `${diffDays}d ago`;
  return new Date(timestamp * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function stashAgeColor(timestamp: number): string {
  const diffDays = Math.floor((Date.now() - timestamp * 1000) / 86_400_000);
  if (diffDays < 1) return 'text-green-400';
  if (diffDays < 7) return 'text-status-modified';
  return 'text-status-deleted';
}

interface StashPanelProps {
  repoId: string;
  stashes: StashEntryInfo[];
  onPush: (message: string) => void;
  onPop: (idx: number) => void;
  onApply: (idx: number) => void;
  onDrop: (idx: number) => void;
  onClear: () => void;
  isPushing: boolean;
  isClearing: boolean;
  /** True while any stash mutation (pop/apply/drop) is in-flight. */
  isMutating: boolean;
}

type StashConfirmAction =
  | { kind: 'pop'; index: number }
  | { kind: 'apply'; index: number }
  | { kind: 'drop'; index: number }
  | { kind: 'clear' };

function StashPanel({ repoId, stashes, onPush, onPop, onApply, onDrop, onClear, isPushing, isClearing, isMutating }: StashPanelProps) {
  const [showCreate, setShowCreate] = useState(false);
  const [stashMessage, setStashMessage] = useState('');
  const [previewStash, setPreviewStash] = useState<StashEntryInfo | null>(null);
  const [confirmAction, setConfirmAction] = useState<StashConfirmAction | null>(null);

  // Derived state: track the previous isMutating value to detect true→false
  // transitions so we can auto-close the confirm dialog when a mutation settles.
  // This uses the React-sanctioned "updating state during render" pattern — calling
  // setState before returning JSX causes React to discard the render and re-run
  // immediately with the new state, without committing to the DOM.
  const [prevIsMutating, setPrevIsMutating] = useState(isMutating);
  if (prevIsMutating !== isMutating) {
    setPrevIsMutating(isMutating);
    if (prevIsMutating && !isMutating && confirmAction !== null) {
      // Mutation just settled — close the confirm dialog.
      setConfirmAction(null);
    }
  }

  function handlePush() {
    onPush(stashMessage.trim() || 'WIP');
    setStashMessage('');
    setShowCreate(false);
  }

  return (
    <div className="border-t border-border pt-2">
      <div className="flex items-center justify-between px-3 py-1">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-text-muted">
          <Archive size={13} />
          Stash
        </h3>
        <div className="flex items-center gap-0.5">
          {stashes.length > 0 && (
            <button
              onClick={() => setConfirmAction({ kind: 'clear' })}
              disabled={isClearing}
              className="rounded p-0.5 text-text-muted transition-colors hover:text-status-deleted disabled:opacity-50"
              title="Clear all stashes"
              aria-label="Clear all stashes"
            >
              <Eraser size={13} />
            </button>
          )}
          <button
            onClick={() => setShowCreate(!showCreate)}
            className="rounded p-0.5 text-text-muted transition-colors hover:text-accent"
          >
            {showCreate ? <X size={13} /> : <Plus size={13} />}
          </button>
        </div>
      </div>

      {showCreate && (
        <div className="flex gap-1 px-3 pb-2">
          <input
            value={stashMessage}
            onChange={(e) => setStashMessage(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handlePush()}
            placeholder="Stash message (WIP)"
            aria-label="Stash message"
            className="flex-1 rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          <button
            onClick={handlePush}
            disabled={isPushing}
            className="rounded bg-accent px-2 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
          >
            Stash
          </button>
        </div>
      )}

      {stashes.length === 0 && (
        <div className="flex flex-col items-center gap-1.5 px-3 py-4 text-center">
          <Archive size={24} className="text-text-muted/30" />
          <p className="text-xs font-medium text-text-muted">No stashes yet</p>
          <p className="text-[11px] text-text-muted/70 leading-relaxed">
            Stash your uncommitted changes to switch context without committing.
          </p>
          <button
            onClick={() => setShowCreate(true)}
            className="mt-1 flex items-center gap-1 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
          >
            <Plus size={10} />
            Stash changes
          </button>
        </div>
      )}

      <div className="space-y-px px-2">
        {stashes.map((stash) => (
          <div
            key={`stash-${stash.commit_id}`}
            className="group rounded border border-transparent px-2 py-2 transition-colors hover:border-border hover:bg-surface-hover"
          >
            {/* Top row: index + message + actions */}
            <div className="flex items-start gap-1.5">
              <Archive size={12} className="mt-0.5 flex-shrink-0 text-text-muted" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-xs text-text-secondary leading-tight">{stash.message}</p>
                <p className="font-mono text-[10px] text-text-muted">
                  stash@{'{' + stash.index + '}'}
                </p>
              </div>
              <div className="flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                <button
                  onClick={() => setPreviewStash(stash)}
                  className="rounded p-0.5 text-text-muted hover:text-accent"
                  title="Preview diff"
                  aria-label={`Preview stash ${stash.index}`}
                >
                  <Eye size={11} />
                </button>
                <button
                  onClick={() => setConfirmAction({ kind: 'pop', index: stash.index })}
                  disabled={isMutating}
                  className="rounded p-0.5 text-text-muted hover:text-accent disabled:cursor-not-allowed disabled:opacity-40"
                  title="Pop (apply and remove)"
                  aria-label={`Pop stash ${stash.index}`}
                >
                  <Play size={11} />
                </button>
                <button
                  onClick={() => setConfirmAction({ kind: 'apply', index: stash.index })}
                  disabled={isMutating}
                  className="rounded p-0.5 text-text-muted hover:text-accent disabled:cursor-not-allowed disabled:opacity-40"
                  title="Apply (keep stash)"
                  aria-label={`Apply stash ${stash.index}`}
                >
                  <Copy size={11} />
                </button>
                <button
                  onClick={() => setConfirmAction({ kind: 'drop', index: stash.index })}
                  disabled={isMutating}
                  className="rounded p-0.5 text-text-muted hover:text-status-deleted disabled:cursor-not-allowed disabled:opacity-40"
                  title="Drop (discard permanently)"
                  aria-label={`Drop stash ${stash.index}`}
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>

            {/* Bottom row: age indicator */}
            {stash.timestamp > 0 && (
              <div className="mt-1 flex items-center gap-1 pl-[20px]">
                <Clock size={9} className="flex-shrink-0 text-text-muted/60" />
                <span
                  className={`text-[10px] ${stashAgeColor(stash.timestamp)}`}
                  title={new Date(stash.timestamp * 1000).toLocaleString()}
                >
                  {formatStashAge(stash.timestamp)}
                </span>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Confirmation modal */}
      {confirmAction && (
        <div className={`mx-3 mt-2 rounded border p-2 ${
          confirmAction.kind === 'drop' || confirmAction.kind === 'clear'
            ? 'border-status-deleted/30 bg-status-deleted/5'
            : 'border-accent/30 bg-accent/5'
        }`}>
          <p className="text-xs text-text-secondary">
            {confirmAction.kind === 'clear'
              ? `Clear all ${String(stashes.length)} stash${stashes.length !== 1 ? 'es' : ''}? This is irreversible.`
              : confirmAction.kind === 'drop'
                ? `Drop stash@{${String(confirmAction.index)}}? This is irreversible.`
                : confirmAction.kind === 'pop'
                  ? `Pop stash@{${String(confirmAction.index)}}? This will apply and remove the stash.`
                  : `Apply stash@{${String(confirmAction.index)}}? This will apply the stash without removing it.`}
          </p>
          <div className="mt-2 flex gap-1">
            <button
              disabled={isMutating}
              onClick={() => {
                if (confirmAction.kind === 'clear') {
                  onClear();
                } else if (confirmAction.kind === 'drop') {
                  onDrop(confirmAction.index);
                } else if (confirmAction.kind === 'pop') {
                  onPop(confirmAction.index);
                } else {
                  onApply(confirmAction.index);
                }
                // Do not close the dialog here — the parent closes it on success.
              }}
              className={`rounded px-2 py-1 text-xs font-medium disabled:cursor-not-allowed disabled:opacity-50 ${
                confirmAction.kind === 'drop' || confirmAction.kind === 'clear'
                  ? 'bg-status-deleted/20 text-status-deleted hover:bg-status-deleted/30'
                  : 'bg-accent text-navy-950 hover:bg-accent-light'
              }`}
            >
              {isMutating
                ? '...'
                : confirmAction.kind === 'clear'
                  ? 'Clear all'
                  : confirmAction.kind === 'drop'
                    ? 'Drop'
                    : confirmAction.kind === 'pop'
                      ? 'Pop'
                      : 'Apply'}
            </button>
            <button
              disabled={isMutating}
              onClick={() => setConfirmAction(null)}
              className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary disabled:opacity-50"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {previewStash && (
        <StashPreview
          repoId={repoId}
          stash={previewStash}
          onPop={onPop}
          onApply={onApply}
          onClose={() => setPreviewStash(null)}
        />
      )}
    </div>
  );
}

export default StashPanel;
