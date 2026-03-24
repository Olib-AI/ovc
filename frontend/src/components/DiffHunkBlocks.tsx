import { useMemo, useState, useCallback } from 'react';
import type React from 'react';
import DOMPurify from 'dompurify';
import type { DiffHunk, DiffLine } from '../api/types.ts';
import { detectLanguage, highlightLines } from '../utils/highlight.ts';
import { Clipboard, Check, MessageSquarePlus, X } from 'lucide-react';

/**
 * A segment within a diff line: either unchanged text or the changed region.
 */
interface DiffSegment {
  text: string;
  changed: boolean;
}

/**
 * Word-level diff between two strings.
 *
 * Strategy: find the longest common prefix and suffix at word-token granularity,
 * then mark the middle region as changed.  This handles the common cases (renamed
 * identifiers, changed values, updated import paths) without a full LCS which would
 * be O(n²) and expensive for long lines.
 *
 * "Word token" here means contiguous runs of alphanumeric/underscore chars or
 * individual punctuation characters — this gives better granularity than splitting
 * on whitespace alone.
 */
function tokenize(s: string): string[] {
  // Split into word-like tokens and individual punctuation/whitespace chars
  return s.match(/\w+|[^\w]/g) ?? [];
}

function computeIntraLineDiff(del: string, add: string): { delSegments: DiffSegment[]; addSegments: DiffSegment[] } {
  const delTokens = tokenize(del);
  const addTokens = tokenize(add);

  let prefixLen = 0;
  const minLen = Math.min(delTokens.length, addTokens.length);
  while (prefixLen < minLen && delTokens[prefixLen] === addTokens[prefixLen]) {
    prefixLen++;
  }

  let suffixLen = 0;
  while (
    suffixLen < minLen - prefixLen &&
    delTokens[delTokens.length - 1 - suffixLen] === addTokens[addTokens.length - 1 - suffixLen]
  ) {
    suffixLen++;
  }

  function buildSegments(tokens: string[]): DiffSegment[] {
    const changedEnd = tokens.length - suffixLen;
    const segments: DiffSegment[] = [];
    if (prefixLen > 0) {
      segments.push({ text: tokens.slice(0, prefixLen).join(''), changed: false });
    }
    if (changedEnd > prefixLen) {
      segments.push({ text: tokens.slice(prefixLen, changedEnd).join(''), changed: true });
    }
    if (suffixLen > 0) {
      segments.push({ text: tokens.slice(changedEnd).join(''), changed: false });
    }
    return segments;
  }

  return {
    delSegments: buildSegments(delTokens),
    addSegments: buildSegments(addTokens),
  };
}

/**
 * Render a sequence of DiffSegments as a React element with inline highlights.
 * The `changedClassName` controls the background of the changed words.
 */
function renderSegments(segments: DiffSegment[], changedClassName: string): React.ReactNode {
  if (segments.length === 0) return '\u00A0';
  // Check if there is actually a change worth highlighting (skip if entire line is marked changed)
  const allChanged = segments.every((s) => s.changed);
  if (allChanged) {
    return segments.map((s) => s.text).join('') || '\u00A0';
  }
  return segments.map((seg, i) =>
    seg.changed ? (
      <mark key={i} className={changedClassName}>
        {seg.text}
      </mark>
    ) : (
      seg.text
    ),
  );
}

export interface SplitRow {
  leftNum: string;
  leftContent: string;
  leftClass: string;
  leftSegments: DiffSegment[] | null;
  leftHunkIndex: number;
  rightNum: string;
  rightContent: string;
  rightClass: string;
  rightSegments: DiffSegment[] | null;
  rightHunkIndex: number;
}

interface LineRow {
  rowClass: string;
  oldNum: string;
  newNum: string;
  prefix: string;
}

// eslint-disable-next-line react-refresh/only-export-components
export function computeLineRow(line: DiffLine, oldLine: number, newLine: number): LineRow {
  switch (line.kind) {
    case 'addition':
      return {
        rowClass: 'bg-diff-add-bg/50 text-diff-add-text',
        oldNum: '',
        newNum: String(newLine),
        prefix: '+',
      };
    case 'deletion':
      return {
        rowClass: 'bg-diff-del-bg/50 text-diff-del-text',
        oldNum: String(oldLine),
        newNum: '',
        prefix: '-',
      };
    case 'context':
    default:
      return {
        rowClass: '',
        oldNum: String(oldLine),
        newNum: String(newLine),
        prefix: ' ',
      };
  }
}

// eslint-disable-next-line react-refresh/only-export-components
export function buildSplitRows(hunk: DiffHunk): SplitRow[] {
  const rows: SplitRow[] = [];
  let oldLine = hunk.old_start;
  let newLine = hunk.new_start;
  let i = 0;
  const lines = hunk.lines;

  while (i < lines.length) {
    const line = lines[i];

    if (line.kind === 'context') {
      rows.push({
        leftNum: String(oldLine),
        leftContent: line.content,
        leftClass: '',
        leftSegments: null,
        leftHunkIndex: i,
        rightNum: String(newLine),
        rightContent: line.content,
        rightClass: '',
        rightSegments: null,
        rightHunkIndex: i,
      });
      oldLine++;
      newLine++;
      i++;
    } else {
      const delIndices: number[] = [];
      const addIndices: number[] = [];
      const deletions: DiffLine[] = [];
      const additions: DiffLine[] = [];

      while (i < lines.length && lines[i].kind === 'deletion') {
        delIndices.push(i);
        deletions.push(lines[i]);
        i++;
      }
      while (i < lines.length && lines[i].kind === 'addition') {
        addIndices.push(i);
        additions.push(lines[i]);
        i++;
      }

      const maxLen = Math.max(deletions.length, additions.length);
      for (let j = 0; j < maxLen; j++) {
        const del = j < deletions.length ? deletions[j] : undefined;
        const add = j < additions.length ? additions[j] : undefined;

        // Compute intra-line word diff when there is a 1-to-1 pair
        let delSegments: DiffSegment[] | null = null;
        let addSegments: DiffSegment[] | null = null;
        if (del && add) {
          const intra = computeIntraLineDiff(del.content, add.content);
          delSegments = intra.delSegments;
          addSegments = intra.addSegments;
        }

        rows.push({
          leftNum: del ? String(oldLine) : '',
          leftContent: del ? del.content : '',
          leftClass: del ? 'bg-diff-del-bg/50 text-diff-del-text' : '',
          leftSegments: delSegments,
          leftHunkIndex: j < delIndices.length ? delIndices[j] : -1,
          rightNum: add ? String(newLine) : '',
          rightContent: add ? add.content : '',
          rightClass: add ? 'bg-diff-add-bg/50 text-diff-add-text' : '',
          rightSegments: addSegments,
          rightHunkIndex: j < addIndices.length ? addIndices[j] : -1,
        });

        if (del) oldLine++;
        if (add) newLine++;
      }
    }
  }

  return rows;
}

/**
 * Highlight the content lines of a diff hunk. We join all the lines (stripped
 * of diff prefixes) into a single source block so highlight.js correctly
 * handles multi-line constructs (block comments, template strings). The result
 * is a Map from the line's content string to the highlighted HTML for that line.
 *
 * We build the map keyed by (lineIndex) to handle duplicate content lines.
 */
function useHunkHighlight(hunk: DiffHunk, filePath?: string): Map<number, string> | null {
  return useMemo(() => {
    if (!filePath) return null;
    const language = detectLanguage(filePath);
    if (!language) return null;

    // Build a single source block from all hunk lines for correct tokenization
    const sourceLines = hunk.lines.map((line) => line.content);
    const source = sourceLines.join('\n');
    const highlighted = highlightLines(source, language);
    if (!highlighted) return null;

    const map = new Map<number, string>();
    for (let i = 0; i < hunk.lines.length; i++) {
      const hl = highlighted[i];
      if (hl !== undefined) {
        map.set(i, DOMPurify.sanitize(hl));
      }
    }
    return map;
  }, [hunk, filePath]);
}

/** Inline comment thread anchored to a line number */
interface LineComment {
  lineNum: number;
  side: 'old' | 'new';
  body: string;
}

/** Inline comment form + display for a single line */
function LineCommentThread({
  lineNum,
  side,
  comments,
  onAdd,
  onClose,
}: {
  lineNum: number;
  side: 'old' | 'new';
  comments: LineComment[];
  onAdd: (lineNum: number, side: 'old' | 'new', body: string) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState('');
  const existing = comments.filter((c) => c.lineNum === lineNum && c.side === side);

  return (
    <tr>
      <td colSpan={4} className="p-0">
        <div className="border-y border-accent/20 bg-navy-800/60 px-4 py-2">
          {existing.map((c, i) => (
            <div key={i} className="mb-2 flex gap-2 text-xs">
              <span className="mt-0.5 flex-shrink-0 text-text-muted">
                <MessageSquarePlus size={12} />
              </span>
              <p className="text-text-secondary">{c.body}</p>
            </div>
          ))}
          <div className="flex items-start gap-2">
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Add a comment..."
              rows={2}
              className="flex-1 resize-none rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
            <div className="flex flex-col gap-1">
              <button
                onClick={() => {
                  if (draft.trim()) {
                    onAdd(lineNum, side, draft.trim());
                    setDraft('');
                  }
                }}
                disabled={!draft.trim()}
                className="rounded bg-accent px-2 py-1 text-[10px] font-medium text-navy-950 hover:bg-accent-light disabled:opacity-40"
              >
                Save
              </button>
              <button
                onClick={onClose}
                className="rounded p-1 text-text-muted hover:bg-surface-hover hover:text-text-primary"
                aria-label="Close comment"
              >
                <X size={11} />
              </button>
            </div>
          </div>
        </div>
      </td>
    </tr>
  );
}

interface HunkBlockProps {
  hunk: DiffHunk;
  filePath?: string;
  /**
   * Called when the user clicks the @@ hunk header to request more context
   * lines around this hunk. Wiring up the actual expansion is deferred —
   * this prop makes the affordance available without breaking existing callers.
   */
  onExpandContext?: (filePath: string, oldStart: number, newStart: number) => void;
}

/**
 * Annotated line for unified view: carries the row metadata plus optional
 * intra-line diff segments for paired deletion/addition lines.
 */
interface AnnotatedLine {
  line: DiffLine;
  oldNum: number;
  newNum: number;
  segments: DiffSegment[] | null;
  /** Index into the original hunk.lines array (for highlight lookup) */
  hunkLineIndex: number;
}

function annotateUnifiedLines(hunk: DiffHunk): AnnotatedLine[] {
  const annotated: AnnotatedLine[] = [];
  let oldLine = hunk.old_start;
  let newLine = hunk.new_start;
  let i = 0;
  const lines = hunk.lines;

  while (i < lines.length) {
    const line = lines[i];
    if (line.kind === 'context') {
      annotated.push({ line, oldNum: oldLine, newNum: newLine, segments: null, hunkLineIndex: i });
      oldLine++;
      newLine++;
      i++;
    } else {
      // Collect a contiguous block of deletions then additions
      const deletionStart = i;
      while (i < lines.length && lines[i].kind === 'deletion') i++;
      const additionStart = i;
      while (i < lines.length && lines[i].kind === 'addition') i++;

      const delLines = lines.slice(deletionStart, additionStart);
      const addLines = lines.slice(additionStart, i);

      // Emit deletions
      for (let j = 0; j < delLines.length; j++) {
        const paired = j < addLines.length;
        const segments = paired
          ? computeIntraLineDiff(delLines[j].content, addLines[j].content).delSegments
          : null;
        annotated.push({ line: delLines[j], oldNum: oldLine, newNum: -1, segments, hunkLineIndex: deletionStart + j });
        oldLine++;
      }
      // Emit additions
      for (let j = 0; j < addLines.length; j++) {
        const paired = j < delLines.length;
        const segments = paired
          ? computeIntraLineDiff(delLines[j].content, addLines[j].content).addSegments
          : null;
        annotated.push({ line: addLines[j], oldNum: -1, newNum: newLine, segments, hunkLineIndex: additionStart + j });
        newLine++;
      }
    }
  }
  return annotated;
}

/** Copy button for a single hunk */
function HunkCopyButton({ hunk }: { hunk: DiffHunk }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    const text = hunk.lines
      .map((l) => {
        const prefix = l.kind === 'addition' ? '+' : l.kind === 'deletion' ? '-' : ' ';
        return `${prefix}${l.content}`;
      })
      .join('\n');
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [hunk]);

  return (
    <button
      onClick={handleCopy}
      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
      title="Copy hunk"
      aria-label="Copy hunk"
    >
      {copied ? <Check size={10} className="text-status-added" /> : <Clipboard size={10} />}
      {copied ? 'Copied' : 'Copy'}
    </button>
  );
}

export function UnifiedHunkBlock({ hunk, filePath, onExpandContext }: HunkBlockProps) {
  const annotated = useMemo(() => annotateUnifiedLines(hunk), [hunk]);
  const hlMap = useHunkHighlight(hunk, filePath);

  // Line commenting state
  const [comments, setComments] = useState<LineComment[]>([]);
  const [activeComment, setActiveComment] = useState<{ lineNum: number; side: 'old' | 'new' } | null>(null);

  const handleAddComment = useCallback((lineNum: number, side: 'old' | 'new', body: string) => {
    setComments((prev) => [...prev, { lineNum, side, body }]);
  }, []);

  const hunkHeader = `@@ -${String(hunk.old_start)},${String(hunk.old_count)} +${String(hunk.new_start)},${String(hunk.new_count)} @@`;

  return (
    <div>
      <div className="flex items-center justify-between bg-diff-hunk-bg pr-2">
        {onExpandContext ? (
          <button
            type="button"
            onClick={() => {
              if (filePath) {
                onExpandContext(filePath, hunk.old_start, hunk.new_start);
              }
            }}
            title="Click to expand context"
            className="group flex-1 px-4 py-1 text-left font-mono text-xs text-accent-dim transition-colors hover:bg-diff-hunk-bg/80 hover:cursor-pointer"
          >
            <span>{hunkHeader}</span>
            <span className="ml-2 opacity-0 transition-opacity group-hover:opacity-60">
              Expand context
            </span>
          </button>
        ) : (
          <div className="flex-1 px-4 py-1 font-mono text-xs text-accent-dim">
            {hunkHeader}
          </div>
        )}
        <HunkCopyButton hunk={hunk} />
      </div>
      <table className="w-full border-collapse font-mono text-[13px] leading-[1.4]">
        <tbody>
          {annotated.map(({ line, oldNum, newNum, segments, hunkLineIndex }, lineIdx) => {
            const row = computeLineRow(
              line,
              oldNum >= 0 ? oldNum : 0,
              newNum >= 0 ? newNum : 0,
            );
            const changedMark =
              line.kind === 'deletion'
                ? 'rounded bg-diff-del-bg text-diff-del-text'
                : 'rounded bg-diff-add-bg text-diff-add-text';
            const hlHtml = hlMap?.get(hunkLineIndex);
            const lineNum = oldNum >= 0 ? oldNum : newNum;
            const side: 'old' | 'new' = oldNum >= 0 ? 'old' : 'new';
            const hasComment = comments.some((c) => c.lineNum === lineNum && c.side === side);
            const isActive = activeComment?.lineNum === lineNum && activeComment.side === side;

            return (
              <>
                <tr
                  key={lineIdx}
                  className={`group/line ${row.rowClass}`}
                >
                  <td
                    className="w-[52px] cursor-pointer select-none border-r border-border/20 px-2 text-right font-mono text-[11px] text-text-muted/40 transition-colors hover:bg-accent/10 hover:text-accent"
                    title="Click to comment on this line"
                    onClick={() =>
                      setActiveComment(isActive ? null : { lineNum, side })
                    }
                  >
                    <span className="flex items-center justify-end gap-1">
                      {(hasComment || isActive) && (
                        <MessageSquarePlus size={9} className="text-accent/60" />
                      )}
                      {oldNum >= 0 ? row.oldNum : ''}
                    </span>
                  </td>
                  <td
                    className="w-[52px] cursor-pointer select-none border-r border-border/20 px-2 text-right font-mono text-[11px] text-text-muted/40 transition-colors hover:bg-accent/10 hover:text-accent"
                    title="Click to comment on this line"
                    onClick={() =>
                      setActiveComment(isActive ? null : { lineNum, side })
                    }
                  >
                    {newNum >= 0 ? row.newNum : ''}
                  </td>
                  <td className="w-5 select-none text-center font-mono text-[11px]">{row.prefix}</td>
                  {segments ? (
                    <td className="whitespace-pre pr-4">
                      {renderSegments(segments, changedMark)}
                    </td>
                  ) : hlHtml !== undefined ? (
                    <td
                      className="whitespace-pre pr-4"
                      dangerouslySetInnerHTML={{ __html: hlHtml || '\u00A0' }}
                    />
                  ) : (
                    <td className="whitespace-pre pr-4">
                      {line.content || '\u00A0'}
                    </td>
                  )}
                </tr>
                {isActive && (
                  <LineCommentThread
                    key={`comment-${lineIdx}`}
                    lineNum={lineNum}
                    side={side}
                    comments={comments}
                    onAdd={handleAddComment}
                    onClose={() => setActiveComment(null)}
                  />
                )}
              </>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function SplitHunkBlock({ hunk, filePath, onExpandContext }: HunkBlockProps) {
  const rows = useMemo(() => buildSplitRows(hunk), [hunk]);
  const hlMap = useHunkHighlight(hunk, filePath);

  const hunkHeader = `@@ -${String(hunk.old_start)},${String(hunk.old_count)} +${String(hunk.new_start)},${String(hunk.new_count)} @@`;

  return (
    <div>
      <div className="flex items-center justify-between bg-diff-hunk-bg pr-2">
        {onExpandContext ? (
          <button
            type="button"
            onClick={() => {
              if (filePath) {
                onExpandContext(filePath, hunk.old_start, hunk.new_start);
              }
            }}
            title="Click to expand context"
            className="group flex-1 px-4 py-1 text-left font-mono text-xs text-accent-dim transition-colors hover:bg-diff-hunk-bg/80 hover:cursor-pointer"
          >
            <span>{hunkHeader}</span>
            <span className="ml-2 opacity-0 transition-opacity group-hover:opacity-60">
              Expand context
            </span>
          </button>
        ) : (
          <div className="flex-1 px-4 py-1 font-mono text-xs text-accent-dim">
            {hunkHeader}
          </div>
        )}
        <HunkCopyButton hunk={hunk} />
      </div>
      <div className="flex">
        <table className="w-1/2 border-collapse border-r border-border font-mono text-[13px] leading-[1.4]">
          <tbody>
            {rows.map((row, idx) => {
              const leftHl = row.leftHunkIndex >= 0 ? hlMap?.get(row.leftHunkIndex) : undefined;
              return (
                <tr key={idx} className={row.leftClass}>
                  <td className="w-[52px] select-none border-r border-border/20 px-2 text-right text-[11px] text-text-muted/40">
                    {row.leftNum}
                  </td>
                  {row.leftSegments ? (
                    <td className="whitespace-pre px-2">
                      {renderSegments(row.leftSegments, 'rounded bg-diff-del-bg text-diff-del-text')}
                    </td>
                  ) : leftHl !== undefined ? (
                    <td
                      className="whitespace-pre px-2"
                      dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(leftHl || '\u00A0') }}
                    />
                  ) : (
                    <td className="whitespace-pre px-2">
                      {row.leftContent || '\u00A0'}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
        <table className="w-1/2 border-collapse font-mono text-[13px] leading-[1.4]">
          <tbody>
            {rows.map((row, idx) => {
              const rightHl = row.rightHunkIndex >= 0 ? hlMap?.get(row.rightHunkIndex) : undefined;
              return (
                <tr key={idx} className={row.rightClass}>
                  <td className="w-[52px] select-none border-r border-border/20 px-2 text-right text-[11px] text-text-muted/40">
                    {row.rightNum}
                  </td>
                  {row.rightSegments ? (
                    <td className="whitespace-pre px-2">
                      {renderSegments(row.rightSegments, 'rounded bg-diff-add-bg text-diff-add-text')}
                    </td>
                  ) : rightHl !== undefined ? (
                    <td
                      className="whitespace-pre px-2"
                      dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(rightHl || '\u00A0') }}
                    />
                  ) : (
                    <td className="whitespace-pre px-2">
                      {row.rightContent || '\u00A0'}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
