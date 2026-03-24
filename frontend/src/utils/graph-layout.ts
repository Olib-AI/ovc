import type { CommitInfo } from '../api/types.ts';

interface GraphNode {
  id: string;
  parentIds: string[];
  lane: number;
  color: number;
  y: number;
}

interface GraphEdge {
  fromId: string;
  toId: string;
  fromLane: number;
  toLane: number;
  color: number;
}

interface GraphLayout {
  nodes: GraphNode[];
  edges: GraphEdge[];
  maxLane: number;
}

const NUM_COLORS = 8;

function computeGraphLayout(commits: CommitInfo[]): GraphLayout {
  const nodes: GraphNode[] = [];
  const edges: GraphEdge[] = [];
  const activeLanes: (string | null)[] = [];
  let nextColor = 0;
  let maxLane = 0;

  const commitColorMap = new Map<string, number>();

  function findLane(commitId: string): number {
    for (let i = 0; i < activeLanes.length; i++) {
      if (activeLanes[i] === commitId) return i;
    }
    return -1;
  }

  function allocateLane(): number {
    for (let i = 0; i < activeLanes.length; i++) {
      if (activeLanes[i] === null) {
        return i;
      }
    }
    activeLanes.push(null);
    return activeLanes.length - 1;
  }

  function getColor(): number {
    const c = nextColor % NUM_COLORS;
    nextColor++;
    return c;
  }

  for (let y = 0; y < commits.length; y++) {
    const commit = commits[y]!;
    let lane = findLane(commit.id);

    if (lane === -1) {
      lane = allocateLane();
      activeLanes[lane] = commit.id;
      commitColorMap.set(commit.id, getColor());
    }

    const color = commitColorMap.get(commit.id) ?? 0;
    activeLanes[lane] = null;

    if (lane > maxLane) maxLane = lane;

    nodes.push({
      id: commit.id,
      parentIds: commit.parent_ids,
      lane,
      color,
      y,
    });

    for (let pi = 0; pi < commit.parent_ids.length; pi++) {
      const parentId = commit.parent_ids[pi]!;
      const existingLane = findLane(parentId);

      if (existingLane !== -1) {
        const edgeColor = commitColorMap.get(parentId) ?? color;
        edges.push({
          fromId: commit.id,
          toId: parentId,
          fromLane: lane,
          toLane: existingLane,
          color: edgeColor,
        });
      } else if (pi === 0) {
        activeLanes[lane] = parentId;
        if (!commitColorMap.has(parentId)) {
          commitColorMap.set(parentId, color);
        }
        edges.push({
          fromId: commit.id,
          toId: parentId,
          fromLane: lane,
          toLane: lane,
          color,
        });
      } else {
        const newLane = allocateLane();
        activeLanes[newLane] = parentId;
        const branchColor = getColor();
        commitColorMap.set(parentId, branchColor);
        if (newLane > maxLane) maxLane = newLane;
        edges.push({
          fromId: commit.id,
          toId: parentId,
          fromLane: lane,
          toLane: newLane,
          color: branchColor,
        });
      }
    }
  }

  return { nodes, edges, maxLane };
}

export { computeGraphLayout };
export type { GraphNode, GraphEdge, GraphLayout };
