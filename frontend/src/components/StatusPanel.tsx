import type { StatusResponse, DiffResponse } from '../api/types.ts';
import { Check, Circle, FileQuestion, ChevronDown, ChevronRight, Plus, Minus, Info, Undo2, Trash2 } from 'lucide-react';
import { memo, useState, type KeyboardEvent } from 'react';
interface StatusPanelProps {
  status: StatusResponse;
  diff: DiffResponse | undefined;
  unstagedDiff?: DiffResponse | undefined;
  selectedFile: string | null;
  onSelectFile: (path: string) => void;
  onStage: (paths: string[]) => void;
  onUnstage: (paths: string[]) => void;
  onRestore: (paths: string[]) => void;
  onClean?: (paths: string[]) => void;
  isStaging: boolean;
  isUnstaging: boolean;
  isRestoring: boolean;
  isCleaning?: boolean;
}

function StatusPanel({
  status,
  diff,
  unstagedDiff,
  selectedFile,
  onSelectFile,
  onStage,
  onUnstage,
  onRestore,
  onClean,
  isStaging,
  isUnstaging,
  isRestoring,
  isCleaning,
}: StatusPanelProps) {
  const [stagedExpanded, setStagedExpanded] = useState(true);
  const [unstagedExpanded, setUnstagedExpanded] = useState(true);
  const [untrackedExpanded, setUntrackedExpanded] = useState(true);
  const [confirmDiscard, setConfirmDiscard] = useState<string | null>(null);
  const [confirmDiscardAll, setConfirmDiscardAll] = useState(false);
  const [confirmClean, setConfirmClean] = useState<string | null>(null);
  const [confirmCleanAll, setConfirmCleanAll] = useState(false);
  const hasChanges =
    status.staged.length > 0 || status.unstaged.length > 0 || status.untracked.length > 0;

  if (!hasChanges) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-text-muted">
        <Check size={32} className="text-status-added" />
        <p className="text-sm">Working tree clean</p>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {/* Change summary */}
      {diff && diff.stats.files_changed > 0 && (
        <div className="flex items-center gap-3 border-b border-border bg-navy-800/40 px-3 py-1.5">
          <span className="text-[11px] text-text-muted">
            {diff.stats.files_changed} file{diff.stats.files_changed !== 1 ? 's' : ''} changed
          </span>
          <span className="flex items-center gap-0.5 text-[11px] text-status-added">
            <Plus size={10} />
            {diff.stats.additions}
          </span>
          <span className="flex items-center gap-0.5 text-[11px] text-status-deleted">
            <Minus size={10} />
            {diff.stats.deletions}
          </span>
        </div>
      )}

      {/* Staged */}
      {status.staged.length > 0 && (
        <div>
          <div
            role="button"
            tabIndex={0}
            onClick={() => setStagedExpanded(!stagedExpanded)}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                setStagedExpanded(!stagedExpanded);
              }
            }}
            className="flex w-full cursor-pointer items-center gap-1.5 border-b border-border bg-diff-add-bg/30 px-3 py-2 text-left"
          >
            {stagedExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <span className="text-xs font-semibold uppercase tracking-wider text-status-added">
              Staged ({status.staged.length})
            </span>
            <div className="ml-auto flex items-center gap-2">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setConfirmDiscardAll(true);
                }}
                className="text-[10px] text-status-deleted hover:text-status-deleted/80"
                title="Restore all staged files to last committed version"
              >
                Restore All
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onUnstage(status.staged.map((s) => s.path));
                }}
                disabled={isUnstaging}
                className="text-[10px] text-text-muted hover:text-text-primary disabled:opacity-50"
              >
                Unstage All
              </button>
            </div>
          </div>
          {stagedExpanded && (
            <div className="divide-y divide-border/30">
              {status.staged.map((entry) => {
                const fileDiff = diff?.files.find((f) => f.path === entry.path);
                const additions = fileDiff
                  ? fileDiff.hunks.reduce(
                      (sum, h) => sum + h.lines.filter((l) => l.kind === 'addition').length,
                      0,
                    )
                  : 0;
                const deletions = fileDiff
                  ? fileDiff.hunks.reduce(
                      (sum, h) => sum + h.lines.filter((l) => l.kind === 'deletion').length,
                      0,
                    )
                  : 0;
                const isSelected = selectedFile === entry.path;
                const fileName = entry.path.split('/').pop() ?? entry.path;
                const dirPath = entry.path.includes('/') ? entry.path.slice(0, entry.path.lastIndexOf('/')) : '';

                return (
                  <div
                    key={entry.path}
                    role="button"
                    tabIndex={0}
                    onClick={() => onSelectFile(entry.path)}
                    onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        onSelectFile(entry.path);
                      }
                    }}
                    className={`group/staged flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-surface-hover ${
                      isSelected ? 'bg-surface-hover' : ''
                    }`}
                    title={entry.path}
                  >
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onUnstage([entry.path]);
                      }}
                      disabled={isUnstaging}
                      className="flex-shrink-0 rounded text-status-added hover:text-text-primary disabled:opacity-50"
                      title="Unstage"
                    >
                      <Check size={14} />
                    </button>
                    <StatusBadge status={entry.status} />
                    <span className="min-w-0 flex-1 overflow-hidden font-mono text-xs">
                      <span className="block truncate text-text-secondary">{fileName}</span>
                      {dirPath && (
                        <span className="block truncate text-[10px] text-text-muted">{dirPath}</span>
                      )}
                    </span>
                    {(additions > 0 || deletions > 0) && (
                      <span className="flex flex-shrink-0 items-center gap-1 text-[10px]">
                        {additions > 0 && (
                          <span className="text-status-added">+{additions}</span>
                        )}
                        {deletions > 0 && (
                          <span className="text-status-deleted">-{deletions}</span>
                        )}
                      </span>
                    )}
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setConfirmDiscard(entry.path);
                      }}
                      className="flex-shrink-0 rounded p-0.5 text-text-muted opacity-0 transition-opacity hover:text-status-deleted group-hover/staged:opacity-100"
                      title="Restore to last committed version"
                      aria-label={`Restore ${entry.path} to last committed version`}
                    >
                      <Undo2 size={12} />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Unstaged */}
      {!status.has_workdir && status.unstaged.length === 0 && status.untracked.length === 0 && (
        <div className="flex items-start gap-2 border-b border-border px-3 py-2">
          <Info size={14} className="mt-0.5 flex-shrink-0 text-text-muted" />
          <div className="text-[11px] leading-relaxed text-text-muted">
            <p>No working directory access. Stage files via CLI:</p>
            <code className="mt-1 block rounded bg-navy-950 px-2 py-1 font-mono text-[10px] text-text-secondary">
              ovc add &lt;file&gt;
            </code>
          </div>
        </div>
      )}
      {status.unstaged.length > 0 && (
        <div>
          <div
            role="button"
            tabIndex={0}
            onClick={() => setUnstagedExpanded(!unstagedExpanded)}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                setUnstagedExpanded(!unstagedExpanded);
              }
            }}
            className="flex w-full cursor-pointer items-center gap-1.5 border-b border-border bg-diff-del-bg/30 px-3 py-2 text-left"
          >
            {unstagedExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <span className="text-xs font-semibold uppercase tracking-wider text-status-deleted">
              Unstaged ({status.unstaged.length})
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onStage(status.unstaged.map((s) => s.path));
              }}
              disabled={isStaging}
              className="ml-auto text-[10px] text-text-muted hover:text-text-primary disabled:opacity-50"
            >
              Stage All
            </button>
          </div>
          {unstagedExpanded && (
            <div className="divide-y divide-border/30">
              {status.unstaged.map((entry) => {
                const fileName = entry.path.split('/').pop() ?? entry.path;
                const dirPath = entry.path.includes('/') ? entry.path.slice(0, entry.path.lastIndexOf('/')) : '';
                const isSelected = selectedFile === entry.path;
                const uFileDiff = unstagedDiff?.files.find((f) => f.path === entry.path);
                const uAdditions = uFileDiff
                  ? uFileDiff.hunks.reduce(
                      (sum, h) => sum + h.lines.filter((l) => l.kind === 'addition').length,
                      0,
                    )
                  : 0;
                const uDeletions = uFileDiff
                  ? uFileDiff.hunks.reduce(
                      (sum, h) => sum + h.lines.filter((l) => l.kind === 'deletion').length,
                      0,
                    )
                  : 0;

                return (
                  <div
                    key={entry.path}
                    role="button"
                    tabIndex={0}
                    onClick={() => onSelectFile(entry.path)}
                    onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        onSelectFile(entry.path);
                      }
                    }}
                    className={`flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-surface-hover ${
                      isSelected ? 'bg-surface-hover' : ''
                    }`}
                    title={entry.path}
                  >
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onStage([entry.path]);
                      }}
                      disabled={isStaging}
                      className="flex-shrink-0 rounded text-status-deleted hover:text-text-primary disabled:opacity-50"
                      title="Stage"
                    >
                      <Circle size={14} />
                    </button>
                    <StatusBadge status={entry.status} />
                    <span className="min-w-0 flex-1 overflow-hidden font-mono text-xs">
                      <span className="block truncate text-text-secondary">{fileName}</span>
                      {dirPath && (
                        <span className="block truncate text-[10px] text-text-muted">{dirPath}</span>
                      )}
                    </span>
                    {(uAdditions > 0 || uDeletions > 0) && (
                      <span className="flex flex-shrink-0 items-center gap-1 text-[10px]">
                        {uAdditions > 0 && (
                          <span className="text-status-added">+{uAdditions}</span>
                        )}
                        {uDeletions > 0 && (
                          <span className="text-status-deleted">-{uDeletions}</span>
                        )}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Untracked */}
      {status.untracked.length > 0 && (
        <div>
          <div
            role="button"
            tabIndex={0}
            onClick={() => setUntrackedExpanded(!untrackedExpanded)}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                setUntrackedExpanded(!untrackedExpanded);
              }
            }}
            className="flex w-full cursor-pointer items-center gap-1.5 border-b border-border px-3 py-2 text-left"
          >
            {untrackedExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <span className="text-xs font-semibold uppercase tracking-wider text-text-muted">
              Untracked ({status.untracked.length})
            </span>
            <div className="ml-auto flex items-center gap-2">
              {onClean && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmCleanAll(true);
                  }}
                  disabled={isCleaning}
                  className="text-[10px] text-status-deleted hover:text-status-deleted/80 disabled:opacity-50"
                >
                  Clean All
                </button>
              )}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onStage(status.untracked);
                }}
                disabled={isStaging}
                className="text-[10px] text-text-muted hover:text-text-primary disabled:opacity-50"
              >
                Stage All
              </button>
            </div>
          </div>
          {untrackedExpanded && (
            <div>
              {/* Info banner + prominent Stage All button */}
              <div className="flex items-center gap-2 border-b border-border/30 bg-navy-800/30 px-3 py-2">
                <Info size={12} className="flex-shrink-0 text-text-muted" />
                <span className="flex-1 text-[11px] text-text-muted">
                  New files — stage to include in next commit
                </span>
                <button
                  onClick={() => onStage(status.untracked)}
                  disabled={isStaging}
                  className="flex items-center gap-1 rounded bg-accent/15 px-2 py-1 text-[11px] font-medium text-accent transition-colors hover:bg-accent/25 disabled:opacity-50"
                >
                  <Plus size={11} />
                  Stage All New Files
                </button>
              </div>
              <div className="divide-y divide-border/30">
                {status.untracked.map((path) => {
                  const fileName = path.split('/').pop() ?? path;
                  const dirPath = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';

                  return (
                    <div
                      key={path}
                      className="group/untracked flex items-center gap-2 px-3 py-1.5 hover:bg-surface-hover"
                      title={path}
                    >
                      <button
                        onClick={() => onStage([path])}
                        disabled={isStaging}
                        className="flex-shrink-0 rounded text-text-muted hover:text-status-added disabled:opacity-50"
                        title="Stage"
                      >
                        <Plus size={14} />
                      </button>
                      <FileQuestion size={14} className="flex-shrink-0 text-text-muted" />
                      <span className="min-w-0 flex-1 overflow-hidden font-mono text-xs">
                        <span className="block truncate text-text-muted">{fileName}</span>
                        {dirPath && (
                          <span className="block truncate text-[10px] text-text-muted/60">{dirPath}</span>
                        )}
                      </span>
                      {onClean && (
                        <button
                          onClick={() => setConfirmClean(path)}
                          disabled={isCleaning}
                          className="flex-shrink-0 rounded p-0.5 text-text-muted opacity-0 transition-opacity hover:text-status-deleted group-hover/untracked:opacity-100 disabled:opacity-50"
                          title="Delete file"
                          aria-label={`Delete ${path}`}
                        >
                          <Trash2 size={12} />
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      )}
      {/* Confirm discard single file */}
      {confirmDiscard && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="fixed inset-0 bg-navy-950/70" onClick={() => setConfirmDiscard(null)} />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-4 shadow-2xl">
            <p className="mb-4 text-sm text-text-primary">
              Revert <span className="font-mono text-accent">{confirmDiscard}</span> to last committed version?
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDiscard(null)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onRestore([confirmDiscard]);
                  setConfirmDiscard(null);
                }}
                disabled={isRestoring}
                className="rounded bg-status-deleted/20 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/30 disabled:opacity-50"
              >
                Restore
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm clean single file */}
      {confirmClean && onClean && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="fixed inset-0 bg-navy-950/70" onClick={() => setConfirmClean(null)} />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-4 shadow-2xl">
            <p className="mb-4 text-sm text-text-primary">
              Permanently delete <span className="font-mono text-accent">{confirmClean}</span>? This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmClean(null)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onClean([confirmClean]);
                  setConfirmClean(null);
                }}
                disabled={isCleaning}
                className="rounded bg-status-deleted/20 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/30 disabled:opacity-50"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm clean all */}
      {confirmCleanAll && onClean && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="fixed inset-0 bg-navy-950/70" onClick={() => setConfirmCleanAll(false)} />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-4 shadow-2xl">
            <p className="mb-4 text-sm text-text-primary">
              Permanently delete all {status.untracked.length} untracked file{status.untracked.length !== 1 ? 's' : ''}? This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmCleanAll(false)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onClean(status.untracked);
                  setConfirmCleanAll(false);
                }}
                disabled={isCleaning}
                className="rounded bg-status-deleted/20 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/30 disabled:opacity-50"
              >
                Delete All
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm discard all */}
      {confirmDiscardAll && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div className="fixed inset-0 bg-navy-950/70" onClick={() => setConfirmDiscardAll(false)} />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-4 shadow-2xl">
            <p className="mb-4 text-sm text-text-primary">
              Revert all staged files to last committed version ({status.staged.length} files)?
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDiscardAll(false)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  onRestore(status.staged.map((s) => s.path));
                  setConfirmDiscardAll(false);
                }}
                disabled={isRestoring}
                className="rounded bg-status-deleted/20 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/30 disabled:opacity-50"
              >
                Restore All
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  let colorClass: string;
  let label: string;

  switch (status) {
    case 'added':
      colorClass = 'bg-status-added/15 text-status-added';
      label = 'A';
      break;
    case 'modified':
      colorClass = 'bg-status-modified/15 text-status-modified';
      label = 'M';
      break;
    case 'deleted':
      colorClass = 'bg-status-deleted/15 text-status-deleted';
      label = 'D';
      break;
    case 'renamed':
      colorClass = 'bg-purple-400/15 text-purple-400';
      label = 'R';
      break;
    case 'copied':
      colorClass = 'bg-blue-400/15 text-blue-400';
      label = 'C';
      break;
    case 'conflicted':
      colorClass = 'bg-orange-400/15 text-orange-400';
      label = '!';
      break;
    default:
      colorClass = 'bg-text-muted/15 text-text-muted';
      label = '?';
  }

  return (
    <span className={`w-5 rounded px-1 text-center font-mono text-[10px] font-bold ${colorClass}`}>
      {label}
    </span>
  );
}

function arraysShallowEqual<T>(a: readonly T[], b: readonly T[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

function statusEqual(a: StatusResponse, b: StatusResponse): boolean {
  return (
    arraysShallowEqual(a.staged, b.staged) &&
    arraysShallowEqual(a.unstaged, b.unstaged) &&
    arraysShallowEqual(a.untracked, b.untracked) &&
    a.branch === b.branch &&
    a.has_workdir === b.has_workdir
  );
}

function diffStatsEqual(a: DiffResponse | undefined, b: DiffResponse | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.stats.files_changed === b.stats.files_changed &&
    a.stats.additions === b.stats.additions &&
    a.stats.deletions === b.stats.deletions &&
    a.files.length === b.files.length
  );
}

const MemoizedStatusPanel = memo(StatusPanel, (prev, next) => {
  return (
    statusEqual(prev.status, next.status) &&
    diffStatsEqual(prev.diff, next.diff) &&
    diffStatsEqual(prev.unstagedDiff, next.unstagedDiff) &&
    prev.selectedFile === next.selectedFile &&
    prev.onSelectFile === next.onSelectFile &&
    prev.onStage === next.onStage &&
    prev.onUnstage === next.onUnstage &&
    prev.onRestore === next.onRestore &&
    prev.onClean === next.onClean &&
    prev.isStaging === next.isStaging &&
    prev.isUnstaging === next.isUnstaging &&
    prev.isRestoring === next.isRestoring &&
    prev.isCleaning === next.isCleaning
  );
});

export default MemoizedStatusPanel;
