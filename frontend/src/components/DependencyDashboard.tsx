import { useState, useMemo, useCallback } from 'react';
import {
  Package,
  RefreshCw,
  CheckCircle,
  AlertTriangle,
  ArrowRight,
  ChevronDown,
  ChevronRight,
  Filter,
  GitMerge,
  GitPullRequest,
  Trash2,
  Check,
  Loader2,
} from 'lucide-react';
import {
  useDependencies,
  useDependencyProposals,
  useCreateDependencyUpdates,
  useDeleteDependencyProposal,
  useMergeDependencyProposal,
  useCreatePrFromProposal,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import type { DependencyStatus, ManifestReport, ProposalInfo, UpdateType } from '../api/types.ts';

interface DependencyDashboardProps {
  repoId: string;
}

type SortMode = 'severity' | 'name' | 'file';

const UPDATE_TYPE_ORDER: Record<UpdateType, number> = {
  major: 0,
  minor: 1,
  patch: 2,
  unknown: 3,
  up_to_date: 4,
};

function UpdateBadge({ type }: { type: UpdateType }) {
  const styles: Record<UpdateType, string> = {
    major: 'bg-red-500/15 text-red-400 border border-red-500/30',
    minor: 'bg-amber-500/15 text-amber-400 border border-amber-500/30',
    patch: 'bg-cyan-500/15 text-cyan-400 border border-cyan-500/30',
    up_to_date: 'bg-green-500/15 text-green-400 border border-green-500/30',
    unknown: 'bg-navy-600/50 text-text-muted border border-border',
  };
  const labels: Record<UpdateType, string> = {
    major: 'major',
    minor: 'minor',
    patch: 'patch',
    up_to_date: 'current',
    unknown: 'unknown',
  };
  return (
    <span className={`inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide ${styles[type]}`}>
      {labels[type]}
    </span>
  );
}

function SummaryBadge({
  count,
  label,
  colorClass,
}: {
  count: number;
  label: string;
  colorClass: string;
}) {
  if (count === 0) return null;
  return (
    <span className={`inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs font-semibold ${colorClass}`}>
      {count} {label}
    </span>
  );
}

interface ManifestGroupProps {
  manifest: ManifestReport;
  showUpToDate: boolean;
  showDevOnly: boolean | null;
  sortMode: SortMode;
}

function ManifestGroup({ manifest, showUpToDate, showDevOnly, sortMode }: ManifestGroupProps) {
  const [collapsed, setCollapsed] = useState(false);

  const filtered = useMemo(() => {
    let deps = manifest.dependencies;
    if (!showUpToDate) {
      deps = deps.filter((d) => d.update_type !== 'up_to_date');
    }
    if (showDevOnly === true) {
      deps = deps.filter((d) => d.dev);
    } else if (showDevOnly === false) {
      deps = deps.filter((d) => !d.dev);
    }
    return deps;
  }, [manifest.dependencies, showUpToDate, showDevOnly]);

  const sorted = useMemo(() => {
    return [...filtered].sort((a, b) => {
      if (sortMode === 'severity') {
        const diff = UPDATE_TYPE_ORDER[a.update_type] - UPDATE_TYPE_ORDER[b.update_type];
        return diff !== 0 ? diff : a.name.localeCompare(b.name);
      }
      if (sortMode === 'name') return a.name.localeCompare(b.name);
      // 'file' — maintain original order within group, sort by dev status
      return Number(a.dev) - Number(b.dev);
    });
  }, [filtered, sortMode]);

  if (filtered.length === 0) return null;

  const updateCount = filtered.filter((d) => d.update_type !== 'up_to_date' && d.update_type !== 'unknown').length;

  return (
    <div className="rounded-lg border border-border bg-navy-900">
      <button
        onClick={() => setCollapsed((c) => !c)}
        className="flex w-full items-center gap-2 px-4 py-3 text-left"
        aria-expanded={!collapsed}
      >
        {collapsed ? (
          <ChevronRight size={14} className="flex-shrink-0 text-text-muted" />
        ) : (
          <ChevronDown size={14} className="flex-shrink-0 text-text-muted" />
        )}
        <Package size={13} className="flex-shrink-0 text-accent" />
        <span className="text-xs font-semibold text-text-primary">{manifest.file}</span>
        <span className="rounded bg-navy-700 px-1.5 py-0.5 text-[10px] font-medium text-text-secondary">
          {manifest.package_manager}
        </span>
        <span className="ml-auto text-[11px] text-text-muted">
          {filtered.length} dep{filtered.length !== 1 ? 's' : ''}
          {updateCount > 0 && (
            <span className="ml-1 text-amber-400">· {updateCount} update{updateCount !== 1 ? 's' : ''}</span>
          )}
        </span>
      </button>

      {!collapsed && (
        <div className="border-t border-border divide-y divide-border/50">
          {sorted.map((dep) => (
            <DependencyRow key={dep.name} dep={dep} />
          ))}
        </div>
      )}
    </div>
  );
}

function DependencyRow({ dep }: { dep: DependencyStatus }) {
  const isUpToDate = dep.update_type === 'up_to_date';
  const hasUpdate = !isUpToDate && dep.update_type !== 'unknown';

  return (
    <div className="flex items-center gap-3 px-4 py-2 hover:bg-surface/30 transition-colors">
      {/* Package name */}
      <div className="flex items-center gap-1.5 min-w-0 w-40 flex-shrink-0">
        <span className={`font-mono text-xs font-medium truncate ${isUpToDate ? 'text-text-secondary' : 'text-text-primary'}`}>
          {dep.name}
        </span>
        {dep.dev && (
          <span className="rounded border border-navy-600 bg-navy-700/50 px-1 py-0.5 text-[9px] font-medium uppercase tracking-wide text-text-muted flex-shrink-0">
            dev
          </span>
        )}
      </div>

      {/* Version */}
      <div className="flex-1 min-w-0">
        {hasUpdate ? (
          <div className="flex items-center gap-2 font-mono text-xs">
            <span className="text-text-muted">{dep.current_version}</span>
            <ArrowRight size={10} className="text-accent/60 flex-shrink-0" />
            <span className="text-accent font-semibold">{dep.latest_version}</span>
          </div>
        ) : (
          <span className="font-mono text-xs text-text-secondary">{dep.current_version}</span>
        )}
      </div>

      {/* Status badge */}
      <div className="flex-shrink-0">
        <UpdateBadge type={dep.update_type} />
      </div>
    </div>
  );
}

// ─── Proposal card ───────────────────────────────────────────────────────────

interface ProposalCardProps {
  proposal: ProposalInfo;
  onMerge: (branch: string) => void;
  onDismiss: (branch: string) => void;
  onCreatePr: (branch: string) => void;
  isMerging: boolean;
  isDismissing: boolean;
  isCreatingPr: boolean;
}

function ProposalCard({ proposal, onMerge, onDismiss, onCreatePr, isMerging, isDismissing, isCreatingPr }: ProposalCardProps) {
  const [confirmDismiss, setConfirmDismiss] = useState(false);

  // Extract human-readable version info from branch name as a best-effort
  // fallback — the proposals list endpoint doesn't include from/to versions,
  // only the full branch name (e.g. deps/cargo/tokio-1.50.0).
  const branchLabel = proposal.branch.replace(/^deps\/[^/]+\//, '');

  return (
    <div
      className={`rounded-lg border bg-navy-900 p-3 transition-colors ${
        proposal.mergeable
          ? 'border-border hover:border-status-added/30'
          : 'border-border hover:border-status-deleted/30'
      }`}
    >
      <div className="flex items-start gap-3">
        {/* Status icon */}
        <div
          className={`mt-0.5 flex-shrink-0 rounded-full p-1 ${
            proposal.mergeable
              ? 'bg-status-added/10 text-status-added'
              : 'bg-status-deleted/10 text-status-deleted'
          }`}
        >
          {proposal.mergeable ? <GitMerge size={13} /> : <AlertTriangle size={13} />}
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-mono text-xs font-semibold text-text-primary">
              {proposal.dependency ?? branchLabel}
            </span>
            {proposal.file && (
              <span className="text-[10px] text-text-muted">{proposal.file}</span>
            )}
            <span className="font-mono text-[10px] text-text-muted">{branchLabel}</span>
          </div>

          {/* Merge status */}
          <div className="mt-1.5 flex items-center gap-2 flex-wrap">
            {proposal.mergeable ? (
              <span className="inline-flex items-center gap-1 rounded border border-status-added/30 bg-status-added/10 px-1.5 py-0.5 text-[10px] font-semibold text-status-added">
                <Check size={9} />
                Ready to merge
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 rounded border border-status-deleted/30 bg-status-deleted/10 px-1.5 py-0.5 text-[10px] font-semibold text-status-deleted">
                <AlertTriangle size={9} />
                Has conflicts
              </span>
            )}
          </div>

          {/* Conflict files */}
          {!proposal.mergeable && proposal.conflict_files.length > 0 && (
            <ul className="mt-1.5 space-y-0.5">
              {proposal.conflict_files.map((f) => (
                <li key={f} className="flex items-center gap-1 font-mono text-[10px] text-status-deleted">
                  <AlertTriangle size={9} className="flex-shrink-0" />
                  {f}
                </li>
              ))}
            </ul>
          )}
        </div>

        {/* Actions */}
        <div className="flex flex-shrink-0 items-center gap-1.5">
          {proposal.mergeable && !confirmDismiss && (
            <button
              onClick={() => onMerge(proposal.branch)}
              disabled={isMerging || isDismissing || isCreatingPr}
              className="flex items-center gap-1 rounded border border-status-added/40 bg-status-added/10 px-2 py-1 text-[11px] font-medium text-status-added transition-colors hover:bg-status-added/20 disabled:opacity-50"
            >
              {isMerging ? <Loader2 size={11} className="animate-spin" /> : <GitMerge size={11} />}
              {isMerging ? 'Merging...' : 'Merge'}
            </button>
          )}

          {!confirmDismiss && (
            <button
              onClick={() => onCreatePr(proposal.branch)}
              disabled={isMerging || isDismissing || isCreatingPr}
              className="flex items-center gap-1 rounded border border-cyan-500/40 bg-cyan-500/10 px-2 py-1 text-[11px] font-medium text-cyan-400 transition-colors hover:bg-cyan-500/20 disabled:opacity-50"
            >
              {isCreatingPr ? <Loader2 size={11} className="animate-spin" /> : <GitPullRequest size={11} />}
              {isCreatingPr ? 'Creating...' : 'Create PR'}
            </button>
          )}

          {!confirmDismiss ? (
            <button
              onClick={() => setConfirmDismiss(true)}
              disabled={isMerging || isDismissing}
              className="flex items-center gap-1 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-muted transition-colors hover:border-status-deleted/40 hover:text-status-deleted disabled:opacity-50"
              title="Dismiss this proposal"
            >
              {isDismissing ? <Loader2 size={11} className="animate-spin" /> : <Trash2 size={11} />}
              Dismiss
            </button>
          ) : (
            <div className="flex items-center gap-1">
              <button
                onClick={() => {
                  onDismiss(proposal.branch);
                  setConfirmDismiss(false);
                }}
                disabled={isDismissing}
                className="rounded border border-status-deleted/40 bg-status-deleted/10 px-2 py-1 text-[11px] font-medium text-status-deleted transition-colors hover:bg-status-deleted/20 disabled:opacity-50"
              >
                {isDismissing ? 'Deleting...' : 'Confirm'}
              </button>
              <button
                onClick={() => setConfirmDismiss(false)}
                className="rounded px-1.5 py-1 text-[11px] text-text-muted transition-colors hover:text-text-primary"
              >
                Cancel
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Proposals section ────────────────────────────────────────────────────────

interface ProposalsSectionProps {
  repoId: string;
}

function ProposalsSection({ repoId }: ProposalsSectionProps) {
  const toast = useToast();
  const { data, isLoading, isError, error } = useDependencyProposals(repoId);
  const deleteMutation = useDeleteDependencyProposal(repoId);
  const mergeMutation = useMergeDependencyProposal(repoId);
  const createPrMutation = useCreatePrFromProposal(repoId);

  // Track per-branch pending operations to show individual loading states
  const [mergingBranch, setMergingBranch] = useState<string | null>(null);
  const [dismissingBranch, setDismissingBranch] = useState<string | null>(null);
  const [creatingPrBranch, setCreatingPrBranch] = useState<string | null>(null);
  const [isMergingAll, setIsMergingAll] = useState(false);

  const proposals = data?.proposals ?? [];
  const mergeableProposals = proposals.filter((p) => p.mergeable);

  const handleMerge = useCallback(
    (branch: string) => {
      setMergingBranch(branch);
      mergeMutation.mutate(branch, {
        onSuccess: (result) => {
          if (result.status === 'merged') {
            toast.success(`Merged ${branch} successfully`);
          } else {
            toast.warning(`Merge of ${branch} resulted in conflicts`);
          }
        },
        onError: (err) => {
          toast.error(
            `Failed to merge ${branch}: ${err instanceof Error ? err.message : 'Unknown error'}`,
          );
        },
        onSettled: () => {
          setMergingBranch(null);
        },
      });
    },
    [mergeMutation, toast],
  );

  const handleDismiss = useCallback(
    (branch: string) => {
      setDismissingBranch(branch);
      deleteMutation.mutate(branch, {
        onSuccess: () => {
          toast.success(`Dismissed proposal: ${branch}`);
        },
        onError: (err) => {
          toast.error(
            `Failed to dismiss ${branch}: ${err instanceof Error ? err.message : 'Unknown error'}`,
          );
        },
        onSettled: () => {
          setDismissingBranch(null);
        },
      });
    },
    [deleteMutation, toast],
  );

  const handleCreatePr = useCallback(
    (branch: string) => {
      setCreatingPrBranch(branch);
      createPrMutation.mutate(branch, {
        onSuccess: (pr) => {
          toast.success(`Created PR #${pr.number}: ${pr.title}`);
        },
        onError: (err) => {
          toast.error(
            `Failed to create PR for ${branch}: ${err instanceof Error ? err.message : 'Unknown error'}`,
          );
        },
        onSettled: () => {
          setCreatingPrBranch(null);
        },
      });
    },
    [createPrMutation, toast],
  );

  const handleMergeAll = useCallback(async () => {
    if (mergeableProposals.length === 0) return;
    setIsMergingAll(true);
    let merged = 0;
    let failed = 0;
    for (const proposal of mergeableProposals) {
      try {
        const result = await mergeMutation.mutateAsync(proposal.branch);
        if (result.status === 'merged') {
          merged++;
        } else {
          failed++;
        }
      } catch {
        failed++;
      }
    }
    setIsMergingAll(false);
    if (failed === 0) {
      toast.success(`Merged all ${merged} proposal${merged !== 1 ? 's' : ''} successfully`);
    } else {
      toast.warning(`Merged ${merged}, failed ${failed}`);
    }
  }, [mergeableProposals, mergeMutation, toast]);

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 py-3 text-xs text-text-muted">
        <Loader2 size={13} className="animate-spin" />
        Loading proposals...
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex items-start gap-2 rounded border border-status-deleted/30 bg-diff-del-bg/20 p-3">
        <AlertTriangle size={14} className="mt-0.5 flex-shrink-0 text-status-deleted" />
        <p className="text-xs text-status-deleted">
          {error instanceof Error ? error.message : 'Failed to load proposals'}
        </p>
      </div>
    );
  }

  if (proposals.length === 0) {
    return (
      <p className="py-2 text-xs text-text-muted">
        No update proposals yet. Click &quot;Create Update Branches&quot; to auto-generate them.
      </p>
    );
  }

  return (
    <div className="space-y-2">
      {mergeableProposals.length > 1 && (
        <div className="flex justify-end">
          <button
            onClick={() => void handleMergeAll()}
            disabled={isMergingAll || mergeMutation.isPending}
            className="flex items-center gap-1.5 rounded border border-status-added/40 bg-status-added/10 px-3 py-1.5 text-xs font-medium text-status-added transition-colors hover:bg-status-added/20 disabled:opacity-50"
          >
            {isMergingAll ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <GitMerge size={12} />
            )}
            {isMergingAll
              ? 'Merging all...'
              : `Merge All (${mergeableProposals.length})`}
          </button>
        </div>
      )}

      {proposals.map((proposal) => (
        <ProposalCard
          key={proposal.branch}
          proposal={proposal}
          onMerge={handleMerge}
          onDismiss={handleDismiss}
          onCreatePr={handleCreatePr}
          isMerging={mergingBranch === proposal.branch || isMergingAll}
          isDismissing={dismissingBranch === proposal.branch}
          isCreatingPr={creatingPrBranch === proposal.branch}
        />
      ))}
    </div>
  );
}

// ─── Main dashboard ───────────────────────────────────────────────────────────

function DependencyDashboard({ repoId }: DependencyDashboardProps) {
  const toast = useToast();
  const { data, isFetching, isError, error, refetch, isFetched } = useDependencies(repoId);
  const createUpdatesMutation = useCreateDependencyUpdates(repoId);
  const [showUpToDate, setShowUpToDate] = useState(true);
  const [showDevOnly, setShowDevOnly] = useState<boolean | null>(null);
  const [sortMode, setSortMode] = useState<SortMode>('severity');
  const [collapsed, setCollapsed] = useState(false);
  const [createSummary, setCreateSummary] = useState<string | null>(null);

  const allUpToDate = isFetched && data !== undefined && data.total_updates === 0;
  const noManifests = isFetched && data !== undefined && data.manifests.length === 0;

  function handleCheck() {
    void refetch();
  }

  function handleCreateBranches() {
    setCreateSummary(null);
    createUpdatesMutation.mutate(undefined, {
      onSuccess: (result) => {
        const summary =
          `Created ${result.created} update branch${result.created !== 1 ? 'es' : ''} ` +
          `(${result.mergeable} mergeable, ${result.conflicting} conflicting)`;
        setCreateSummary(summary);
        toast.success(summary);
      },
      onError: (err) => {
        toast.error(
          `Failed to create update branches: ${err instanceof Error ? err.message : 'Unknown error'}`,
        );
      },
    });
  }

  return (
    <div className="rounded-lg border border-border bg-navy-900 p-4">
      {/* Header row 1: title + badges */}
      <div className="flex items-center gap-2 mb-2">
        <button
          onClick={() => setCollapsed((c) => !c)}
          className="flex items-center gap-2 text-left"
          aria-expanded={!collapsed}
        >
          {collapsed ? (
            <ChevronRight size={14} className="text-text-muted" />
          ) : (
            <ChevronDown size={14} className="text-text-muted" />
          )}
          <Package size={16} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">Dependency Updates</h2>
        </button>

        {isFetched && data && data.total_updates > 0 && (
          <div className="flex items-center gap-1.5 ml-2">
            <SummaryBadge
              count={data.major_updates}
              label="major"
              colorClass="bg-red-500/15 text-red-400"
            />
            <SummaryBadge
              count={data.minor_updates}
              label="minor"
              colorClass="bg-amber-500/15 text-amber-400"
            />
            <SummaryBadge
              count={data.patch_updates}
              label="patch"
              colorClass="bg-cyan-500/15 text-cyan-400"
            />
          </div>
        )}
      </div>

      {/* Header row 2: action buttons */}
      <div className="flex items-center gap-2 mb-3">
        <button
          onClick={handleCheck}
          disabled={isFetching}
          className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
        >
          {isFetching ? (
            <RefreshCw size={13} className="animate-spin" />
          ) : (
            <Package size={13} />
          )}
          {isFetching ? 'Checking...' : 'Check for Updates'}
        </button>

        <button
          onClick={handleCreateBranches}
          disabled={createUpdatesMutation.isPending}
          className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
          title="Auto-discover all outdated dependencies and create a branch for each"
        >
          {createUpdatesMutation.isPending ? (
            <Loader2 size={13} className="animate-spin" />
          ) : (
            <GitPullRequest size={13} />
          )}
          {createUpdatesMutation.isPending ? 'Creating...' : 'Create Update Branches'}
        </button>
      </div>

      {!collapsed && (
        <>
          {/* Create branches success summary */}
          {createSummary !== null && (
            <div className="mb-3 flex items-center gap-2 rounded border border-status-added/30 bg-diff-add-bg/20 px-3 py-2">
              <Check size={13} className="flex-shrink-0 text-status-added" />
              <span className="text-xs text-status-added">{createSummary}</span>
            </div>
          )}

          {/* Error state */}
          {isError && (
            <div className="mb-3 flex items-start gap-3 rounded border border-status-deleted/30 bg-diff-del-bg/20 p-3">
              <AlertTriangle size={15} className="mt-0.5 flex-shrink-0 text-status-deleted" />
              <div className="min-w-0 flex-1">
                <p className="text-xs font-semibold text-status-deleted">Failed to check dependencies</p>
                <p className="mt-0.5 text-[11px] text-text-muted">
                  {error instanceof Error ? error.message : 'An unexpected error occurred'}
                </p>
              </div>
              <button
                onClick={handleCheck}
                disabled={isFetching}
                className="flex-shrink-0 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
              >
                Retry
              </button>
            </div>
          )}

          {/* Empty — no manifests */}
          {noManifests && (
            <div className="flex flex-col items-center gap-2 py-8">
              <Package size={32} className="text-text-muted/40" />
              <p className="text-sm text-text-muted">No dependency manifests found in this repository</p>
            </div>
          )}

          {/* All up to date */}
          {allUpToDate && !noManifests && (
            <div className="flex items-center gap-2 rounded border border-status-added/30 bg-diff-add-bg/20 px-3 py-2.5">
              <CheckCircle size={14} className="text-status-added" />
              <span className="text-xs font-medium text-status-added">All dependencies are up to date!</span>
            </div>
          )}

          {/* Results */}
          {isFetched && data && data.manifests.length > 0 && data.total_updates > 0 && (
            <>
              {/* Summary row */}
              <div className="mb-3 flex items-center gap-1.5 flex-wrap">
                <span className="text-[11px] text-text-muted mr-1">
                  {data.total_updates} update{data.total_updates !== 1 ? 's' : ''} available
                </span>
                <SummaryBadge
                  count={data.major_updates}
                  label="major"
                  colorClass="bg-red-500/15 text-red-400"
                />
                <SummaryBadge
                  count={data.minor_updates}
                  label="minor"
                  colorClass="bg-amber-500/15 text-amber-400"
                />
                <SummaryBadge
                  count={data.patch_updates}
                  label="patch"
                  colorClass="bg-cyan-500/15 text-cyan-400"
                />
              </div>

              {/* Filter + sort controls */}
              <div className="mb-3 flex items-center gap-2 flex-wrap">
                <Filter size={12} className="text-text-muted" />
                <button
                  onClick={() => setShowUpToDate((v) => !v)}
                  className={`rounded border px-2 py-0.5 text-[11px] transition-colors ${
                    showUpToDate
                      ? 'border-accent/40 bg-accent/10 text-accent'
                      : 'border-border bg-surface text-text-muted hover:border-accent/30 hover:text-text-secondary'
                  }`}
                >
                  Up-to-date
                </button>
                <button
                  onClick={() =>
                    setShowDevOnly((v) => (v === false ? null : v === null ? true : null))
                  }
                  className={`rounded border px-2 py-0.5 text-[11px] transition-colors ${
                    showDevOnly !== null
                      ? 'border-accent/40 bg-accent/10 text-accent'
                      : 'border-border bg-surface text-text-muted hover:border-accent/30 hover:text-text-secondary'
                  }`}
                  title={
                    showDevOnly === null
                      ? 'Filter: all deps'
                      : showDevOnly
                      ? 'Filter: dev only'
                      : 'Filter: prod only'
                  }
                >
                  {showDevOnly === null ? 'All deps' : showDevOnly ? 'Dev only' : 'Prod only'}
                </button>
                <div className="ml-auto flex items-center gap-1">
                  <span className="text-[11px] text-text-muted">Sort:</span>
                  {(['severity', 'name', 'file'] as const).map((mode) => (
                    <button
                      key={mode}
                      onClick={() => setSortMode(mode)}
                      className={`rounded border px-2 py-0.5 text-[11px] capitalize transition-colors ${
                        sortMode === mode
                          ? 'border-accent/40 bg-accent/10 text-accent'
                          : 'border-border bg-surface text-text-muted hover:border-accent/30 hover:text-text-secondary'
                      }`}
                    >
                      {mode}
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}

          {/* Manifest groups */}
          {isFetched && data && data.manifests.length > 0 && (
            <div className="space-y-3">
              {data.manifests.map((manifest) => (
                <ManifestGroup
                  key={manifest.file}
                  manifest={manifest}
                  showUpToDate={showUpToDate}
                  showDevOnly={showDevOnly}
                  sortMode={sortMode}
                />
              ))}
            </div>
          )}

          {/* Idle state — not yet fetched */}
          {!isFetched && !isFetching && !isError && (
            <p className="py-2 text-xs text-text-muted">
              Click &quot;Check for Updates&quot; to scan dependency manifests against package registries.
            </p>
          )}

          {/* Update Proposals section */}
          <div className="mt-5">
            <div className="mb-3 flex items-center gap-2 border-t border-border pt-4">
              <GitPullRequest size={14} className="text-accent" />
              <h3 className="text-sm font-semibold text-text-primary">Update Proposals</h3>
            </div>
            <ProposalsSection repoId={repoId} />
          </div>
        </>
      )}
    </div>
  );
}

export default DependencyDashboard;
