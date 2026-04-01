import { useState } from 'react';
import type { DiffResponse } from '../api/types.ts';
import { FileCode, Plus, Minus, Columns2, Rows3, Clipboard } from 'lucide-react';
import { UnifiedHunkBlock, SplitHunkBlock } from './DiffHunkBlocks.tsx';

type ViewMode = 'unified' | 'split';

interface ChangesDiffViewProps {
  stagedDiff: DiffResponse | undefined;
  unstagedDiff: DiffResponse | undefined;
  selectedFile: string | null;
  isLoading: boolean;
}

function ChangesDiffView({ stagedDiff, unstagedDiff, selectedFile, isLoading }: ChangesDiffViewProps) {
  const [viewMode, setViewMode] = useState<ViewMode>('unified');
  const [diffSourceOverride, setDiffSourceOverride] = useState<'staged' | 'unstaged' | null>(null);

  if (!selectedFile) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-text-muted">
        <FileCode size={40} strokeWidth={1.5} />
        <p className="text-sm">Select a changed file to view its diff</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-5 w-5 animate-spin rounded-full border-2 border-accent border-t-transparent" />
      </div>
    );
  }

  const stagedFileDiff = stagedDiff?.files.find((f) => f.path === selectedFile);
  const unstagedFileDiff = unstagedDiff?.files.find((f) => f.path === selectedFile);
  const hasBoth = !!stagedFileDiff && !!unstagedFileDiff;
  const defaultSource: 'staged' | 'unstaged' = stagedFileDiff ? 'staged' : 'unstaged';
  const diffSource: 'staged' | 'unstaged' = hasBoth && diffSourceOverride ? diffSourceOverride : defaultSource;
  const fileDiff = diffSource === 'staged' ? stagedFileDiff : unstagedFileDiff;

  if (!fileDiff) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-text-muted">
        <FileCode size={32} strokeWidth={1.5} />
        <p className="text-sm">No changes found for this file</p>
      </div>
    );
  }

  const additions = fileDiff.hunks.reduce(
    (sum, h) => sum + h.lines.filter((l) => l.kind === 'addition').length,
    0,
  );
  const deletions = fileDiff.hunks.reduce(
    (sum, h) => sum + h.lines.filter((l) => l.kind === 'deletion').length,
    0,
  );

  const rawDiffText = fileDiff.hunks
    .flatMap((hunk) => {
      const header = `@@ -${hunk.old_start},${hunk.old_count} +${hunk.new_start},${hunk.new_count} @@`;
      const lines = hunk.lines.map((line) => {
        const prefix = line.kind === 'addition' ? '+' : line.kind === 'deletion' ? '-' : ' ';
        return `${prefix}${line.content}`;
      });
      return [header, ...lines];
    })
    .join('\n');

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* File header */}
      <div className="flex items-center gap-3 border-b border-border bg-navy-800/50 px-4 py-2">
        <FileDiffBadge status={fileDiff.status} />
        {hasBoth ? (
          <div className="flex rounded border border-border">
            <button
              onClick={() => setDiffSourceOverride('staged')}
              className={`px-1.5 py-0.5 text-[10px] font-bold uppercase transition-colors ${
                diffSource === 'staged'
                  ? 'bg-status-added/15 text-status-added'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              Staged
            </button>
            <button
              onClick={() => setDiffSourceOverride('unstaged')}
              className={`px-1.5 py-0.5 text-[10px] font-bold uppercase transition-colors ${
                diffSource === 'unstaged'
                  ? 'bg-status-modified/15 text-status-modified'
                  : 'text-text-muted hover:text-text-secondary'
              }`}
            >
              Unstaged
            </button>
          </div>
        ) : (
          <span className={`flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase ${
            diffSource === 'staged'
              ? 'bg-status-added/15 text-status-added'
              : 'bg-status-modified/15 text-status-modified'
          }`}>
            {diffSource}
          </span>
        )}
        <span className="min-w-0 flex-1 truncate font-mono text-sm text-text-primary">
          {fileDiff.path}
        </span>
        <span className="flex items-center gap-0.5 text-xs text-status-added">
          <Plus size={11} />
          {additions}
        </span>
        <span className="flex items-center gap-0.5 text-xs text-status-deleted">
          <Minus size={11} />
          {deletions}
        </span>

        <button
          onClick={() => {
            void navigator.clipboard.writeText(rawDiffText);
          }}
          className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
          title="Copy diff"
          aria-label="Copy diff"
        >
          <Clipboard size={13} />
        </button>

        <div className="flex rounded border border-border">
          <button
            onClick={() => setViewMode('unified')}
            className={`flex items-center gap-1 px-2 py-1 text-[10px] transition-colors ${
              viewMode === 'unified' ? 'bg-accent/15 text-accent' : 'text-text-muted hover:text-text-secondary'
            }`}
            title="Unified view"
          >
            <Rows3 size={11} />
            Unified
          </button>
          <button
            onClick={() => setViewMode('split')}
            className={`flex items-center gap-1 px-2 py-1 text-[10px] transition-colors ${
              viewMode === 'split' ? 'bg-accent/15 text-accent' : 'text-text-muted hover:text-text-secondary'
            }`}
            title="Split view"
          >
            <Columns2 size={11} />
            Split
          </button>
        </div>
      </div>

      {/* Hunks */}
      <div className="flex-1 overflow-auto">
        {fileDiff.hunks.length === 0 ? (
          <div className="flex h-32 items-center justify-center text-sm text-text-muted">
            Binary file or empty diff
          </div>
        ) : viewMode === 'unified' ? (
          fileDiff.hunks.map((hunk, hunkIdx) => (
            <UnifiedHunkBlock key={hunkIdx} hunk={hunk} />
          ))
        ) : (
          fileDiff.hunks.map((hunk, hunkIdx) => (
            <SplitHunkBlock key={hunkIdx} hunk={hunk} />
          ))
        )}
      </div>
    </div>
  );
}

function FileDiffBadge({ status }: { status: string }) {
  let colorClass: string;
  let label: string;

  switch (status) {
    case 'added':
      colorClass = 'bg-status-added/15 text-status-added';
      label = 'Added';
      break;
    case 'modified':
      colorClass = 'bg-status-modified/15 text-status-modified';
      label = 'Modified';
      break;
    case 'deleted':
      colorClass = 'bg-status-deleted/15 text-status-deleted';
      label = 'Deleted';
      break;
    default:
      colorClass = 'bg-text-muted/15 text-text-muted';
      label = status;
  }

  return (
    <span className={`flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase ${colorClass}`}>
      {label}
    </span>
  );
}

export default ChangesDiffView;
