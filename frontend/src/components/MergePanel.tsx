import { useState } from 'react';
import { AlertTriangle, Check, Info } from 'lucide-react';
import type { MergeResponse } from '../api/types.ts';

interface MergePanelProps {
  result: MergeResponse;
  onDismiss: () => void;
  onAbort?: () => void;
  onStageAndContinue?: () => void;
}

function MergePanel({ result, onDismiss, onAbort, onStageAndContinue }: MergePanelProps) {
  const isConflict = result.status === 'conflict';
  const isClean = result.status === 'clean';
  const isUpToDate = result.status === 'already_up_to_date';
  const [confirmAbort, setConfirmAbort] = useState(false);
  const [conflictsResolved, setConflictsResolved] = useState(false);

  return (
    <div
      className={`rounded-md border p-4 ${
        isConflict
          ? 'border-status-deleted/30 bg-diff-del-bg/30'
          : isClean
            ? 'border-status-added/30 bg-diff-add-bg/30'
            : 'border-border bg-navy-800'
      }`}
    >
      <div className="flex items-start gap-3">
        {isConflict && <AlertTriangle size={18} className="mt-0.5 text-status-deleted" />}
        {isClean && <Check size={18} className="mt-0.5 text-status-added" />}
        {isUpToDate && <Info size={18} className="mt-0.5 text-accent" />}

        <div className="flex-1">
          {isConflict && (
            <>
              <p className="text-sm font-medium text-status-deleted">Merge conflicts detected</p>
              <p className="mt-1 text-xs text-text-muted">
                Resolve conflicts in your editor, then click &quot;Continue&quot; to stage and commit, or &quot;Abort&quot; to discard the merge.
              </p>
              <ul className="mt-2 space-y-1">
                {result.conflict_files.map((path) => (
                  <li key={path} className="flex items-center gap-1.5 font-mono text-xs text-text-secondary">
                    <AlertTriangle size={12} className="flex-shrink-0 text-status-deleted" />
                    {path}
                  </li>
                ))}
              </ul>
              <code className="mt-3 block rounded bg-navy-950 px-2 py-1.5 font-mono text-[11px] text-text-secondary">
                ovc add {result.conflict_files.join(' ')} &amp;&amp; ovc commit -m &quot;Merge resolved&quot;
              </code>

              {(onAbort || onStageAndContinue) && (
                <div className="mt-3 space-y-2">
                  {onStageAndContinue && (
                    <div className="rounded border border-border bg-navy-950 p-2.5 space-y-2">
                      <p className="text-[11px] text-text-muted">
                        Make sure all conflicts are resolved before staging
                      </p>
                      <label className="flex cursor-pointer items-center gap-2 text-xs text-text-secondary select-none">
                        <input
                          type="checkbox"
                          checked={conflictsResolved}
                          onChange={(e) => setConflictsResolved(e.target.checked)}
                          className="h-3.5 w-3.5 rounded border-border accent-accent"
                        />
                        I have resolved all conflicts
                      </label>
                    </div>
                  )}
                  <div className="flex items-center gap-2">
                    {onAbort && !confirmAbort && (
                      <button
                        onClick={() => setConfirmAbort(true)}
                        className="rounded border border-status-deleted/30 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/10"
                      >
                        Abort Merge
                      </button>
                    )}
                    {onAbort && confirmAbort && (
                      <div className="flex items-center gap-2 rounded border border-status-deleted/30 bg-status-deleted/5 px-3 py-1.5">
                        <span className="text-xs text-text-secondary">Are you sure? This discards all merge changes.</span>
                        <button
                          onClick={() => {
                            onAbort();
                            setConfirmAbort(false);
                          }}
                          className="rounded bg-status-deleted px-2 py-1 text-xs font-medium text-white hover:opacity-90"
                        >
                          Confirm Abort
                        </button>
                        <button
                          onClick={() => setConfirmAbort(false)}
                          className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary"
                        >
                          Cancel
                        </button>
                      </div>
                    )}
                    {onStageAndContinue && !confirmAbort && (
                      <button
                        onClick={onStageAndContinue}
                        disabled={!conflictsResolved}
                        className="rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        Stage All &amp; Continue
                      </button>
                    )}
                  </div>
                </div>
              )}
            </>
          )}

          {isClean && (
            <>
              <p className="text-sm font-medium text-status-added">Merge successful</p>
              {result.commit_id && (
                <p className="mt-1 font-mono text-xs text-text-muted">
                  Commit: {result.commit_id.slice(0, 12)}
                </p>
              )}
            </>
          )}

          {isUpToDate && (
            <p className="text-sm text-text-secondary">Already up to date</p>
          )}

          <button
            onClick={onDismiss}
            className="mt-3 rounded px-3 py-1 text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}

export default MergePanel;
