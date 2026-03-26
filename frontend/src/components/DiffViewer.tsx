import { useState, useMemo, useCallback } from 'react';
import type { DiffResponse, FileDiff, DiffHunk } from '../api/types.ts';
import { ChevronDown, ChevronRight, Plus, Minus, Columns2, Rows3, FoldVertical, UnfoldVertical, Clipboard, List, ChevronsUpDown, Loader2 } from 'lucide-react';
import { UnifiedHunkBlock, SplitHunkBlock } from './DiffHunkBlocks.tsx';
import * as api from '../api/client.ts';
import LlmPanel from './LlmPanel.tsx';
import { useLlmStream, useLlmConfig } from '../hooks/useLlm.ts';

type ViewMode = 'unified' | 'split';

interface DiffViewerProps {
  diff: DiffResponse;
  /** When provided, enables lazy-loading of individual file diffs for files with empty hunks. */
  repoId?: string;
  /** Commit ID for lazy-loading individual file diffs. */
  commitId?: string;
}

/** Stable, URL-safe id for a file's diff block anchor. */
function fileDiffId(path: string): string {
  return `file-diff-${path.replace(/[^a-zA-Z0-9_-]/g, '_')}`;
}

interface FileTocEntry {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

/** Gap between consecutive hunks: how many context lines are between them? */
interface HunkGap {
  /** Index of the hunk that ends before the gap (the hunk just above) */
  afterHunkIdx: number;
  /** Number of unchanged lines in the gap */
  gapLines: number;
}

function computeHunkGaps(hunks: DiffHunk[]): HunkGap[] {
  const gaps: HunkGap[] = [];
  for (let i = 0; i < hunks.length - 1; i++) {
    const current = hunks[i];
    const next = hunks[i + 1];
    const currentEnd = current.new_start + current.new_count;
    const nextStart = next.new_start;
    const gapLines = Math.max(0, nextStart - currentEnd);
    gaps.push({ afterHunkIdx: i, gapLines });
  }
  return gaps;
}

function DiffViewer({ diff, repoId, commitId }: DiffViewerProps) {
  const [viewMode, setViewMode] = useState<ViewMode>('unified');
  const [forceExpanded, setForceExpanded] = useState<boolean | undefined>(undefined);
  const [tocOpen, setTocOpen] = useState(true);

  const rawDiffText = useMemo(() => {
    const parts: string[] = [];
    for (const file of diff.files) {
      parts.push(`--- a/${file.path}`);
      parts.push(`+++ b/${file.path}`);
      for (const hunk of file.hunks) {
        parts.push(`@@ -${hunk.old_start},${hunk.old_count} +${hunk.new_start},${hunk.new_count} @@`);
        for (const line of hunk.lines) {
          const prefix = line.kind === 'addition' ? '+' : line.kind === 'deletion' ? '-' : ' ';
          parts.push(`${prefix}${line.content}`);
        }
      }
    }
    return parts.join('\n');
  }, [diff]);

  // LLM explain diff
  const { data: llmConfig } = useLlmConfig(repoId);
  const llmExplainEnabled = !!repoId && (!!llmConfig?.server_enabled || !!llmConfig?.base_url) && (llmConfig?.enabled_features?.explain_diff ?? true);
  const explainStreamFn = useCallback(
    (signal: AbortSignal) => api.streamExplainDiff(repoId!, rawDiffText, signal),
    [repoId, rawDiffText],
  );
  const explain = useLlmStream(explainStreamFn);

  const tocEntries = useMemo((): FileTocEntry[] => {
    return diff.files.map((file) => {
      // Prefer per-file stats from backend; fall back to counting hunk lines
      const additions = file.additions ?? file.hunks.reduce(
        (sum, h) => sum + h.lines.filter((l) => l.kind === 'addition').length,
        0,
      );
      const deletions = file.deletions ?? file.hunks.reduce(
        (sum, h) => sum + h.lines.filter((l) => l.kind === 'deletion').length,
        0,
      );
      return { path: file.path, status: file.status, additions, deletions };
    });
  }, [diff]);

  const handleLocalToggle = useCallback(() => {
    setForceExpanded(undefined);
  }, []);

  const scrollToFile = useCallback((path: string) => {
    const el = document.getElementById(fileDiffId(path));
    el?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, []);

  if (diff.files.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-text-muted">
        <p className="text-sm">No changes</p>
      </div>
    );
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* File tree sidebar */}
      {tocOpen && (
        <div className="hidden w-56 flex-shrink-0 flex-col border-r border-border bg-navy-800/40 overflow-y-auto sm:flex">
          <div className="border-b border-border px-3 py-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
              Changed Files
            </span>
          </div>
          <ul className="flex flex-col gap-px px-1 py-1.5" role="list" aria-label="Changed files">
            {tocEntries.map((entry) => (
              <li key={entry.path}>
                <button
                  onClick={() => scrollToFile(entry.path)}
                  className="flex w-full items-start gap-2 rounded px-2 py-1.5 text-left transition-colors hover:bg-surface-hover/60"
                >
                  <DiffStatusBadge status={entry.status} />
                  <span className="min-w-0 flex-1 break-all font-mono text-[11px] leading-tight text-text-secondary hover:text-text-primary">
                    {entry.path}
                  </span>
                  <span className="flex flex-shrink-0 flex-col items-end gap-0.5 text-[10px]">
                    {entry.additions > 0 && (
                      <span className="flex items-center gap-0.5 text-status-added">
                        <Plus size={9} />
                        {entry.additions}
                      </span>
                    )}
                    {entry.deletions > 0 && (
                      <span className="flex items-center gap-0.5 text-status-deleted">
                        <Minus size={9} />
                        {entry.deletions}
                      </span>
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Main diff content */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Overall summary toolbar */}
        <div className="sticky top-0 z-10 flex items-center gap-4 border-b border-border bg-navy-900 px-4 py-2">
          <span className="text-xs text-text-muted">
            {diff.stats.files_changed} file{diff.stats.files_changed !== 1 ? 's' : ''} changed
          </span>
          <span className="flex items-center gap-0.5 text-xs text-status-added">
            <Plus size={11} />
            {diff.stats.additions}
          </span>
          <span className="flex items-center gap-0.5 text-xs text-status-deleted">
            <Minus size={11} />
            {diff.stats.deletions}
          </span>

          <div className="ml-auto flex items-center gap-1">
            <button
              onClick={() => setTocOpen((v) => !v)}
              className={`rounded p-1 transition-colors hover:bg-surface-hover ${
                tocOpen ? 'text-accent' : 'text-text-muted hover:text-text-primary'
              }`}
              title={tocOpen ? 'Hide file list' : 'Show file list'}
              aria-label={tocOpen ? 'Hide file list' : 'Show file list'}
              aria-expanded={tocOpen}
            >
              <List size={13} />
            </button>
            <button
              onClick={() => {
                void navigator.clipboard.writeText(rawDiffText);
              }}
              className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Copy raw diff"
              aria-label="Copy raw diff"
            >
              <Clipboard size={13} />
            </button>
            <button
              onClick={() => setForceExpanded(true)}
              className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Expand all"
              aria-label="Expand all"
            >
              <UnfoldVertical size={13} />
            </button>
            <button
              onClick={() => setForceExpanded(false)}
              className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Collapse all"
              aria-label="Collapse all"
            >
              <FoldVertical size={13} />
            </button>
            <div className="ml-1 flex rounded border border-border">
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
        </div>

        {/* LLM Explain Diff */}
        {llmExplainEnabled && (
          <div className="px-4 py-3">
            <LlmPanel
              title="Explain Changes"
              content={explain.content}
              isStreaming={explain.isStreaming}
              error={explain.error}
              onGenerate={explain.start}
              onCancel={explain.cancel}
            />
          </div>
        )}

        {/* File diffs */}
        <div className="flex-1 overflow-y-auto">
          <div className="divide-y divide-border">
            {diff.files.map((file) => (
              <FileDiffBlock
                key={file.path}
                file={file}
                viewMode={viewMode}
                forceExpanded={forceExpanded}
                onLocalToggle={handleLocalToggle}
                repoId={repoId}
                commitId={commitId}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

interface FileDiffBlockProps {
  file: FileDiff;
  viewMode: ViewMode;
  forceExpanded: boolean | undefined;
  onLocalToggle: () => void;
  /** For lazy-loading individual file diffs when hunks are empty. */
  repoId?: string;
  commitId?: string;
}

function FileDiffBlock({ file, viewMode, forceExpanded, onLocalToggle, repoId, commitId }: FileDiffBlockProps) {
  const [expanded, setExpanded] = useState(true);
  // Track which inter-hunk gaps are expanded
  const [expandedGaps, setExpandedGaps] = useState<Set<number>>(new Set());
  // Lazy-loaded hunks for files that came back with stats only
  const [lazyHunks, setLazyHunks] = useState<DiffHunk[] | null>(null);
  const [lazyLoading, setLazyLoading] = useState(false);

  // Sync with parent expand/collapse all
  const isExpanded = forceExpanded !== undefined ? forceExpanded : expanded;

  // Use lazy-loaded hunks if available, otherwise use the file's hunks
  const effectiveHunks = lazyHunks ?? file.hunks;
  const needsLazyLoad = file.hunks.length === 0 && (file.additions ?? 0) + (file.deletions ?? 0) > 0 && !lazyHunks;

  const additions = file.additions ?? effectiveHunks.reduce(
    (sum, h) => sum + h.lines.filter((l) => l.kind === 'addition').length,
    0,
  );
  const deletions = file.deletions ?? effectiveHunks.reduce(
    (sum, h) => sum + h.lines.filter((l) => l.kind === 'deletion').length,
    0,
  );

  const loadFileDiff = useCallback(async () => {
    if (!repoId || !commitId || lazyLoading) return;
    setLazyLoading(true);
    try {
      const result = await api.getCommitFileDiff(repoId, commitId, file.path);
      setLazyHunks(result.hunks);
    } catch {
      // Failed to load — leave as stats only
    } finally {
      setLazyLoading(false);
    }
  }, [repoId, commitId, file.path, lazyLoading]);

  const hunkGaps = useMemo(() => computeHunkGaps(effectiveHunks), [effectiveHunks]);

  const toggleGap = useCallback((afterIdx: number) => {
    setExpandedGaps((prev) => {
      const next = new Set(prev);
      if (next.has(afterIdx)) {
        next.delete(afterIdx);
      } else {
        next.add(afterIdx);
      }
      return next;
    });
  }, []);

  return (
    <div id={fileDiffId(file.path)}>
      <button
        onClick={() => {
          setExpanded(!isExpanded);
          onLocalToggle();
        }}
        className="flex w-full items-center gap-2 bg-navy-800/50 px-4 py-2 text-left transition-colors hover:bg-navy-800"
      >
        {isExpanded ? (
          <ChevronDown size={14} className="text-text-muted" />
        ) : (
          <ChevronRight size={14} className="text-text-muted" />
        )}
        <DiffStatusBadge status={file.status} />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-text-primary">
          {file.path}
        </span>
        <span className="flex flex-shrink-0 items-center gap-2 text-[11px]">
          {additions > 0 && (
            <span className="flex items-center gap-0.5 text-status-added">
              <Plus size={10} />
              {additions}
            </span>
          )}
          {deletions > 0 && (
            <span className="flex items-center gap-0.5 text-status-deleted">
              <Minus size={10} />
              {deletions}
            </span>
          )}
        </span>
      </button>

      {isExpanded && (
        <div className="overflow-x-auto">
          {needsLazyLoad ? (
            <div className="flex h-16 items-center justify-center gap-2">
              {lazyLoading ? (
                <Loader2 size={14} className="animate-spin text-text-muted" />
              ) : (
                <button
                  onClick={loadFileDiff}
                  className="rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-accent"
                >
                  Load diff ({additions + deletions} lines changed)
                </button>
              )}
            </div>
          ) : effectiveHunks.length === 0 ? (
            <div className="flex h-16 items-center justify-center text-xs text-text-muted">
              Binary file or empty diff
            </div>
          ) : viewMode === 'unified' ? (
            effectiveHunks.map((hunk, hunkIdx) => (
              <div key={hunkIdx}>
                <UnifiedHunkBlock hunk={hunk} filePath={file.path} />
                {/* Expand/collapse gap between this hunk and the next */}
                {hunkIdx < effectiveHunks.length - 1 && hunkGaps[hunkIdx] && hunkGaps[hunkIdx].gapLines > 0 && (
                  <button
                    type="button"
                    onClick={() => toggleGap(hunkIdx)}
                    className="flex w-full items-center justify-center gap-2 bg-navy-900/60 py-1.5 text-[11px] text-text-muted transition-colors hover:bg-navy-800 hover:text-text-primary"
                    title={expandedGaps.has(hunkIdx) ? 'Collapse unchanged lines' : `Show ${hunkGaps[hunkIdx].gapLines} unchanged lines`}
                  >
                    <ChevronsUpDown size={11} />
                    {expandedGaps.has(hunkIdx)
                      ? 'Collapse unchanged region'
                      : `Show ${hunkGaps[hunkIdx].gapLines} more unchanged line${hunkGaps[hunkIdx].gapLines !== 1 ? 's' : ''}`}
                  </button>
                )}
              </div>
            ))
          ) : (
            effectiveHunks.map((hunk, hunkIdx) => (
              <div key={hunkIdx}>
                <SplitHunkBlock hunk={hunk} filePath={file.path} />
                {hunkIdx < effectiveHunks.length - 1 && hunkGaps[hunkIdx] && hunkGaps[hunkIdx].gapLines > 0 && (
                  <button
                    type="button"
                    onClick={() => toggleGap(hunkIdx)}
                    className="flex w-full items-center justify-center gap-2 bg-navy-900/60 py-1.5 text-[11px] text-text-muted transition-colors hover:bg-navy-800 hover:text-text-primary"
                    title={expandedGaps.has(hunkIdx) ? 'Collapse unchanged lines' : `Show ${hunkGaps[hunkIdx].gapLines} unchanged lines`}
                  >
                    <ChevronsUpDown size={11} />
                    {expandedGaps.has(hunkIdx)
                      ? 'Collapse unchanged region'
                      : `Show ${hunkGaps[hunkIdx].gapLines} more unchanged line${hunkGaps[hunkIdx].gapLines !== 1 ? 's' : ''}`}
                  </button>
                )}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}

function DiffStatusBadge({ status }: { status: string }) {
  let colorClass = '';
  let label = '';

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
    default:
      colorClass = 'bg-text-muted/15 text-text-muted';
      label = '?';
  }

  return (
    <span className={`rounded px-1.5 py-0.5 text-[10px] font-bold ${colorClass}`}>
      {label}
    </span>
  );
}

export default DiffViewer;
