import { useMemo, useState, useCallback } from 'react';
import type { CommitInfo, BranchInfo, TagInfo } from '../api/types.ts';
import { computeGraphLayout } from '../utils/graph-layout.ts';
import type { GraphNode, GraphEdge } from '../utils/graph-layout.ts';
import ContextMenu from './ContextMenu.tsx';
import type { ContextMenuItem } from './ContextMenu.tsx';
import { Copy, GitBranch, CherryIcon, Tag, StickyNote, RotateCcw, History, Loader2, GitCommitVertical } from 'lucide-react';
import type { ResetMode } from '../api/types.ts';
import { useTheme } from '../contexts/ThemeContext.tsx';

interface CommitGraphProps {
  commits: CommitInfo[];
  branches: BranchInfo[];
  tags: TagInfo[];
  selectedCommitId: string | null;
  onSelectCommit: (id: string) => void;
  onCopyHash?: (commitId: string) => void;
  onCreateBranch?: (commitId: string) => void;
  onCherryPick?: (commitId: string) => void;
  onCreateTag?: (commitId: string) => void;
  onRevert?: (commitId: string) => void;
  onReset?: (commitId: string, mode: ResetMode) => void;
  notedCommitIds?: Set<string>;
  isLoading?: boolean;
}

const ROW_HEIGHT = 32;
const LANE_WIDTH = 16;
const LEFT_PAD = 12;
const NODE_RADIUS = 5;

const LANE_COLORS_DARK = [
  '#06b6d4', // cyan
  '#d946ef', // magenta
  '#eab308', // yellow
  '#22c55e', // green
  '#f97316', // orange
  '#ec4899', // pink
  '#8b5cf6', // purple
  '#3b82f6', // blue
];

const LANE_COLORS_LIGHT = [
  '#0e7490', // darker cyan
  '#a21caf', // darker magenta
  '#a16207', // darker yellow
  '#15803d', // darker green
  '#c2410c', // darker orange
  '#be185d', // darker pink
  '#6d28d9', // darker purple
  '#1d4ed8', // darker blue
];

/**
 * Normalize the signature_status field from the API.
 * Unknown values (e.g. future API additions) are treated as 'unsigned' so the
 * UI never breaks — no badge is shown for unsigned commits.
 */
function normalizeSignatureStatus(
  raw: string,
): 'verified' | 'unverified' | 'unsigned' {
  if (raw === 'verified' || raw === 'unverified') return raw;
  return 'unsigned';
}

function laneX(lane: number): number {
  return LEFT_PAD + lane * LANE_WIDTH + LANE_WIDTH / 2;
}

function rowY(row: number): number {
  return row * ROW_HEIGHT + ROW_HEIGHT / 2;
}

function buildEdgePath(edge: GraphEdge, nodeMap: Map<string, GraphNode>): string {
  const fromNode = nodeMap.get(edge.fromId);
  const toNode = nodeMap.get(edge.toId);
  if (!fromNode || !toNode) return '';

  const x1 = laneX(edge.fromLane);
  const y1 = rowY(fromNode.y);
  const x2 = laneX(edge.toLane);
  const y2 = rowY(toNode.y);

  if (x1 === x2) {
    return `M ${x1} ${y1} L ${x2} ${y2}`;
  }

  const midY = y1 + ROW_HEIGHT / 2;
  return `M ${x1} ${y1} L ${x1} ${midY} Q ${x1} ${midY + 8} ${(x1 + x2) / 2} ${midY + 8} Q ${x2} ${midY + 8} ${x2} ${midY + 16} L ${x2} ${y2}`;
}

function formatRelativeDate(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);
  if (diffSecs < 60) return 'just now';
  const diffMins = Math.floor(diffSecs / 60);
  if (diffMins < 60) return `${String(diffMins)}m ago`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${String(diffHours)}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 30) return `${String(diffDays)}d ago`;
  const diffMonths = Math.floor(diffDays / 30);
  if (diffMonths < 12) return `${String(diffMonths)}mo ago`;
  return `${String(Math.floor(diffMonths / 12))}y ago`;
}

function CommitGraph({
  commits,
  branches,
  tags,
  selectedCommitId,
  onSelectCommit,
  onCopyHash,
  onCreateBranch,
  onCherryPick,
  onCreateTag,
  onRevert,
  onReset,
  notedCommitIds,
  isLoading,
}: CommitGraphProps) {
  const { theme } = useTheme();
  const laneColors = theme === 'light' ? LANE_COLORS_LIGHT : LANE_COLORS_DARK;

  function colorForIndex(idx: number): string {
    return laneColors[idx % laneColors.length]!;
  }

  const [contextMenu, setContextMenu] = useState<{
    position: { x: number; y: number };
    commitId: string;
  } | null>(null);

  const [confirmAction, setConfirmAction] = useState<{
    type: 'revert' | 'reset';
    commitId: string;
  } | null>(null);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, commitId: string) => {
      e.preventDefault();
      setContextMenu({ position: { x: e.clientX, y: e.clientY }, commitId });
    },
    [],
  );

  const contextMenuItems = useMemo((): ContextMenuItem[] => {
    if (!contextMenu) return [];
    const cid = contextMenu.commitId;
    const items: ContextMenuItem[] = [
      {
        label: 'Copy commit hash',
        icon: <Copy size={12} />,
        onClick: () => {
          if (onCopyHash) {
            onCopyHash(cid);
          } else {
            void navigator.clipboard.writeText(cid);
          }
        },
      },
    ];
    if (onCreateBranch) {
      items.push({
        label: 'Create branch here',
        icon: <GitBranch size={12} />,
        onClick: () => onCreateBranch(cid),
      });
    }
    if (onCherryPick) {
      items.push({
        label: 'Cherry-pick onto current',
        icon: <CherryIcon size={12} />,
        onClick: () => onCherryPick(cid),
      });
    }
    if (onCreateTag) {
      items.push({
        label: 'Create tag here',
        icon: <Tag size={12} />,
        onClick: () => onCreateTag(cid),
      });
    }
    if (onRevert) {
      items.push({
        label: 'Revert commit',
        icon: <RotateCcw size={12} />,
        danger: true,
        onClick: () => setConfirmAction({ type: 'revert', commitId: cid }),
      });
    }
    if (onReset) {
      items.push({
        label: 'Reset to this commit',
        icon: <History size={12} />,
        danger: true,
        onClick: () => setConfirmAction({ type: 'reset', commitId: cid }),
      });
    }
    return items;
  }, [contextMenu, onCopyHash, onCreateBranch, onCherryPick, onCreateTag, onRevert, onReset]);
  const layout = useMemo(() => computeGraphLayout(commits), [commits]);

  const nodeMap = useMemo(() => {
    const map = new Map<string, GraphNode>();
    for (const node of layout.nodes) {
      map.set(node.id, node);
    }
    return map;
  }, [layout.nodes]);

  const branchLabels = useMemo(() => {
    const map = new Map<string, BranchInfo[]>();
    for (const b of branches) {
      const existing = map.get(b.commit_id);
      if (existing) {
        existing.push(b);
      } else {
        map.set(b.commit_id, [b]);
      }
    }
    return map;
  }, [branches]);

  const tagLabels = useMemo(() => {
    const map = new Map<string, TagInfo[]>();
    for (const t of tags) {
      const existing = map.get(t.commit_id);
      if (existing) {
        existing.push(t);
      } else {
        map.set(t.commit_id, [t]);
      }
    }
    return map;
  }, [tags]);

  const svgWidth = LEFT_PAD + (layout.maxLane + 2) * LANE_WIDTH;
  const totalHeight = commits.length * ROW_HEIGHT;

  if (isLoading && commits.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-text-muted">
        <Loader2 size={24} className="animate-spin" />
        <p className="text-sm">Loading commits...</p>
      </div>
    );
  }

  if (commits.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-text-muted">
        <GitCommitVertical size={32} className="opacity-40" />
        <div className="text-center">
          <p className="text-sm font-medium">No commits yet</p>
          <p className="mt-0.5 text-xs text-text-muted/70">
            Make your first commit to start tracking history
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {confirmAction && (
        <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-status-deleted/30 bg-status-deleted/5 px-4 py-2">
          {confirmAction.type === 'revert' ? (
            <RotateCcw size={13} className="flex-shrink-0 text-status-deleted" />
          ) : (
            <History size={13} className="flex-shrink-0 text-status-deleted" />
          )}
          <span className="text-xs text-text-secondary">
            {confirmAction.type === 'revert'
              ? <>Revert commit <span className="font-mono text-accent/70">{confirmAction.commitId.slice(0, 12)}</span>? This will create a new commit that undoes the changes.</>
              : <>Reset HEAD to <span className="font-mono text-accent/70">{confirmAction.commitId.slice(0, 12)}</span>? Changes become unstaged (mixed mode).</>
            }
          </span>
          <div className="ml-auto flex items-center gap-2">
            <button
              onClick={() => {
                if (confirmAction.type === 'revert' && onRevert) {
                  onRevert(confirmAction.commitId);
                } else if (confirmAction.type === 'reset' && onReset) {
                  onReset(confirmAction.commitId, 'mixed');
                }
                setConfirmAction(null);
              }}
              className="rounded bg-status-deleted px-2 py-1 text-xs font-medium text-white hover:opacity-90"
            >
              {confirmAction.type === 'revert' ? 'Revert' : 'Reset'}
            </button>
            <button
              onClick={() => setConfirmAction(null)}
              className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary"
            >
              Cancel
            </button>
          </div>
        </div>
      )}
      <div style={{ height: totalHeight, position: 'relative' }}>
        {/* SVG layer for graph lines and nodes */}
        <svg
          width={svgWidth}
          height={totalHeight}
          className="absolute left-0 top-0"
          style={{ pointerEvents: 'none' }}
        >
          {layout.edges.map((edge, i) => (
            <path
              key={`${edge.fromId}-${edge.toId}-${String(i)}`}
              d={buildEdgePath(edge, nodeMap)}
              fill="none"
              stroke={colorForIndex(edge.color)}
              strokeWidth={2}
              strokeOpacity={0.7}
            />
          ))}
          {layout.nodes.map((node) => (
            <circle
              key={node.id}
              cx={laneX(node.lane)}
              cy={rowY(node.y)}
              r={NODE_RADIUS}
              fill={colorForIndex(node.color)}
              stroke="var(--theme-node-stroke)"
              strokeWidth={2}
            />
          ))}
        </svg>

        {/* Row layer for commit info */}
        {commits.map((commit, idx) => {
          const commitBranches = branchLabels.get(commit.id);
          const commitTags = tagLabels.get(commit.id);
          return (
            <button
              key={commit.id}
              onClick={() => onSelectCommit(commit.id)}
              onContextMenu={(e) => handleContextMenu(e, commit.id)}
              className={`absolute flex w-full items-center text-left transition-colors ${
                selectedCommitId === commit.id
                  ? 'bg-accent/10'
                  : 'hover:bg-surface-hover'
              }`}
              style={{
                top: idx * ROW_HEIGHT,
                height: ROW_HEIGHT,
                paddingLeft: svgWidth + 8,
              }}
            >
              <div className="flex min-w-0 flex-1 items-center gap-2">
                {normalizeSignatureStatus(commit.signature_status) === 'verified' && (
                  <span
                    className="inline-flex flex-shrink-0 items-center gap-0.5 rounded border border-green-500/30 bg-green-500/10 px-1.5 py-0 text-[10px] font-semibold leading-4 text-green-400"
                    title={commit.signer_identity ?? commit.signer_fingerprint ?? 'Verified'}
                  >
                    &#x2713; Verified
                  </span>
                )}
                {normalizeSignatureStatus(commit.signature_status) === 'unverified' && (
                  <span
                    className="inline-flex flex-shrink-0 items-center gap-0.5 rounded border border-red-500/30 bg-red-500/10 px-1.5 py-0 text-[10px] font-semibold leading-4 text-red-400"
                    title="Signature could not be verified"
                  >
                    &#x2717; Unverified
                  </span>
                )}
                {commitBranches?.map((b) => (
                  <span
                    key={b.name}
                    title={b.name}
                    className={`inline-flex max-w-[120px] flex-shrink-0 items-center truncate rounded-full px-1.5 py-0 text-[10px] font-semibold leading-4 ${
                      b.is_current
                        ? 'bg-accent/20 text-accent'
                        : 'bg-navy-600/60 text-text-secondary'
                    }`}
                  >
                    {b.name}
                  </span>
                ))}
                {commitTags?.map((t) => (
                  <span
                    key={t.name}
                    title={t.name}
                    className="inline-flex max-w-[100px] flex-shrink-0 items-center truncate rounded border border-yellow-500/30 bg-yellow-500/10 px-1.5 py-0 text-[10px] font-semibold leading-4 text-yellow-400"
                  >
                    {t.name}
                  </span>
                ))}
                {notedCommitIds?.has(commit.id) && (
                  <span
                    className="inline-flex flex-shrink-0 items-center text-yellow-400"
                    title="Has note"
                  >
                    <StickyNote size={12} />
                  </span>
                )}
                <span className="truncate text-xs text-text-primary">
                  {commit.message.split('\n')[0]}
                </span>
              </div>
              <div className="flex flex-shrink-0 items-center gap-3 pr-4 pl-2">
                <span className="text-[11px] text-text-muted">{commit.author.name}</span>
                <span className="text-[11px] text-text-muted">
                  {formatRelativeDate(commit.authored_at)}
                </span>
                <span className="font-mono text-[11px] text-accent/70">{commit.short_id}</span>
              </div>
            </button>
          );
        })}
      </div>

      {contextMenu && contextMenuItems.length > 0 && (
        <ContextMenu
          items={contextMenuItems}
          position={contextMenu.position}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}

export default CommitGraph;
