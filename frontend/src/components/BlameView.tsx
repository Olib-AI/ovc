import { useMemo, useState, useRef, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import type { BlameResponse, BlameLine } from '../api/types.ts';
import { User, Clock, GitCommitVertical } from 'lucide-react';

interface BlameViewProps {
  repoId: string;
  blame: BlameResponse;
}

function formatRelativeTime(timestamp: number): string {
  const now = Date.now() / 1000;
  const diff = now - timestamp;

  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 2592000) return `${Math.floor(diff / 86400)}d ago`;
  if (diff < 31536000) return `${Math.floor(diff / 2592000)}mo ago`;
  return `${Math.floor(diff / 31536000)}y ago`;
}

function formatFullDate(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** Returns true when this line starts a new commit block (differs from the previous line). */
function isFirstInBlock(lines: BlameLine[], index: number): boolean {
  if (index === 0) return true;
  return lines[index]!.commit_id !== lines[index - 1]!.commit_id;
}

interface CommitTooltipProps {
  line: BlameLine;
  anchorRef: React.RefObject<HTMLElement | null>;
}

function CommitTooltip({ line, anchorRef }: CommitTooltipProps) {
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!anchorRef.current) return;
    const rect = anchorRef.current.getBoundingClientRect();
    const tooltipWidth = 280;
    const left = Math.min(rect.right + 8, window.innerWidth - tooltipWidth - 8);
    setPos({ top: rect.top, left });
  }, [anchorRef]);

  if (!pos) return null;

  return (
    <div
      ref={tooltipRef}
      className="pointer-events-none fixed z-50 w-70 rounded-lg border border-border bg-navy-800 shadow-2xl"
      style={{ top: pos.top, left: pos.left, width: 280 }}
    >
      <div className="space-y-1.5 px-3 py-2.5">
        <div className="flex items-center gap-1.5">
          <GitCommitVertical size={12} className="flex-shrink-0 text-accent" />
          <span className="font-mono text-[11px] text-accent">{line.commit_id.slice(0, 16)}</span>
        </div>
        <div className="flex items-center gap-1.5">
          <User size={11} className="flex-shrink-0 text-text-muted" />
          <span className="text-[11px] text-text-secondary">{line.author}</span>
        </div>
        <div className="flex items-center gap-1.5">
          <Clock size={11} className="flex-shrink-0 text-text-muted" />
          <span className="text-[11px] text-text-secondary">{formatFullDate(line.timestamp)}</span>
        </div>
      </div>
    </div>
  );
}

function BlameView({ repoId, blame }: BlameViewProps) {
  const navigate = useNavigate();
  const [hoveredCommitId, setHoveredCommitId] = useState<string | null>(null);
  const [tooltipLine, setTooltipLine] = useState<BlameLine | null>(null);
  const anchorRef = useRef<HTMLElement | null>(null);

  // Build a map of commit_id -> alternating color index for visual grouping
  const commitColorMap = useMemo(() => {
    const map = new Map<string, number>();
    let colorIdx = 0;
    let prevCommitId = '';

    for (const line of blame.lines) {
      if (line.commit_id !== prevCommitId) {
        if (!map.has(line.commit_id)) {
          map.set(line.commit_id, colorIdx % 2);
          colorIdx++;
        }
        prevCommitId = line.commit_id;
      }
    }
    return map;
  }, [blame.lines]);

  function handleMouseEnter(line: BlameLine, el: HTMLElement) {
    setHoveredCommitId(line.commit_id);
    setTooltipLine(line);
    anchorRef.current = el;
  }

  function handleMouseLeave() {
    setHoveredCommitId(null);
    setTooltipLine(null);
    anchorRef.current = null;
  }

  return (
    <div className="h-full overflow-auto">
      {tooltipLine && (
        <CommitTooltip line={tooltipLine} anchorRef={anchorRef} />
      )}
      <table className="w-full border-collapse font-mono text-[13px] leading-5">
        <tbody>
          {blame.lines.map((line, idx) => {
            const colorBand = commitColorMap.get(line.commit_id) ?? 0;
            const isBlockStart = isFirstInBlock(blame.lines, idx);
            const isHovered = hoveredCommitId === line.commit_id;
            return (
              <BlameLineRow
                key={line.line_number}
                line={line}
                colorBand={colorBand}
                isBlockStart={isBlockStart}
                isHovered={isHovered}
                onClickCommit={(commitId) =>
                  navigate(`/repo/${repoId}/history?commit=${commitId}`)
                }
                onMouseEnter={handleMouseEnter}
                onMouseLeave={handleMouseLeave}
              />
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

interface BlameLineRowProps {
  line: BlameLine;
  colorBand: number;
  isBlockStart: boolean;
  isHovered: boolean;
  onClickCommit: (commitId: string) => void;
  onMouseEnter: (line: BlameLine, el: HTMLElement) => void;
  onMouseLeave: () => void;
}

function BlameLineRow({
  line,
  colorBand,
  isBlockStart,
  isHovered,
  onClickCommit,
  onMouseEnter,
  onMouseLeave,
}: BlameLineRowProps) {
  const rowRef = useRef<HTMLTableRowElement>(null);

  // Alternating bands: even commits get slightly lighter bg
  const baseBg = colorBand === 0 ? 'bg-navy-900/40' : 'bg-navy-800/20';
  const hoveredBg = 'bg-accent/8';
  const bgClass = isHovered ? hoveredBg : baseBg;

  const shortHash = line.commit_id.slice(0, 8);

  function handleMouseEnterRow() {
    if (rowRef.current) {
      const td = rowRef.current.querySelector('td');
      onMouseEnter(line, (td as HTMLElement | null) ?? rowRef.current);
    }
  }

  return (
    <tr
      ref={rowRef}
      className={`${bgClass} transition-colors`}
      onMouseEnter={handleMouseEnterRow}
      onMouseLeave={onMouseLeave}
    >
      {/* Commit metadata cell — show full info only on block start, otherwise blank */}
      <td className="w-[280px] select-none border-r border-border/30 px-3 py-0">
        {isBlockStart ? (
          <div className="flex items-center gap-2 text-[11px]">
            <button
              onClick={() => onClickCommit(line.commit_id)}
              className="font-mono text-accent transition-colors hover:text-accent-light hover:underline"
              title={line.commit_id}
            >
              {shortHash}
            </button>
            <span className="max-w-[110px] truncate text-text-muted" title={line.author}>
              {line.author}
            </span>
            <span className="text-text-muted/60 whitespace-nowrap">
              {formatRelativeTime(line.timestamp)}
            </span>
          </div>
        ) : (
          /* Indent continuation lines with a subtle left border in the accent color */
          <div className="ml-1 border-l-2 border-accent/15 pl-2 h-full" />
        )}
      </td>

      {/* Line number */}
      <td className="w-12 select-none border-r border-border/20 px-3 text-right text-[11px] text-text-muted/50">
        {line.line_number}
      </td>

      {/* Code content */}
      <td className="whitespace-pre px-4 text-text-secondary">
        {line.content || '\u00A0'}
      </td>
    </tr>
  );
}

// Pre-computed widths to avoid Math.random() during render (ESLint impure-function rule)
const SKELETON_WIDTHS = [
  '65%', '42%', '78%', '55%', '38%', '70%', '48%', '62%', '35%', '80%',
  '52%', '44%', '68%', '30%', '75%', '57%', '40%', '66%', '50%', '72%',
  '37%', '60%', '45%', '73%', '53%', '39%', '67%', '49%', '76%', '43%',
];

/** Skeleton loader that mimics the blame table layout. */
export function BlameViewSkeleton() {
  const rows = Array.from({ length: 30 }, (_, i) => i);
  return (
    <div className="h-full overflow-hidden">
      <table className="w-full border-collapse font-mono text-[13px] leading-5">
        <tbody>
          {rows.map((i) => (
            <tr key={i} className={i % 2 === 0 ? 'bg-navy-900/40' : 'bg-navy-800/20'}>
              <td className="w-[280px] border-r border-border/30 px-3 py-0.5">
                {i % 4 === 0 ? (
                  <div className="flex items-center gap-2">
                    <div className="h-3 w-16 animate-pulse rounded bg-navy-700/60" />
                    <div className="h-3 w-20 animate-pulse rounded bg-navy-700/40" />
                    <div className="h-3 w-10 animate-pulse rounded bg-navy-700/30" />
                  </div>
                ) : (
                  <div className="ml-1 h-4 border-l-2 border-accent/10" />
                )}
              </td>
              <td className="w-12 border-r border-border/20 px-3 text-right">
                <div className="ml-auto h-3 w-5 animate-pulse rounded bg-navy-700/30" />
              </td>
              <td className="px-4">
                <div
                  className="h-3 animate-pulse rounded bg-navy-700/40"
                  style={{ width: SKELETON_WIDTHS[i % SKELETON_WIDTHS.length] }}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default BlameView;
