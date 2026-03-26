import { useState, useEffect, useRef, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import {
  ArrowLeft,
  ArrowRight,
  GitPullRequest,
  GitMerge,
  Check,
  X,
  Pencil,
  ChevronDown,
  ChevronRight,
  Loader2,
  RefreshCw,
  AlertTriangle,
  CircleDot,
  CheckCircle2,
  XCircle,
  Clock,
  Container,
  ShieldCheck,
} from 'lucide-react';
import {
  usePullRequestView,
  useMergeBranch,
  useGetPullRequest,
  useUpdatePullRequest,
  useMergePullRequest,
  useRunPrChecks,
  useListBranchProtection,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import PullRequestViewComponent from '../components/PullRequestView.tsx';
import ReviewPanel from '../components/ReviewPanel.tsx';
import LlmPanel from '../components/LlmPanel.tsx';
import { useLlmStream, useLlmConfig } from '../hooks/useLlm.ts';
import { streamPrReview } from '../api/client.ts';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import type { MergeStrategy } from '../api/client.ts';
import type { BranchProtectionInfo, PrCheckResult, PrChecks, PrMergeStrategy } from '../api/types.ts';
import axios from 'axios';
import { marked } from 'marked';
import DOMPurify from 'dompurify';

const STATE_BADGE_CLASS: Record<string, string> = {
  open: 'bg-green-500/15 text-green-400',
  closed: 'bg-red-500/15 text-red-400',
  merged: 'bg-purple-500/15 text-purple-400',
};

const MERGE_STRATEGIES: { value: PrMergeStrategy; label: string }[] = [
  { value: 'merge', label: 'Merge commit' },
  { value: 'squash', label: 'Squash and merge' },
  { value: 'rebase', label: 'Rebase and merge' },
];

/** Detect whether the splat param is a PR number or a branch name. */
function parsePrIdentifier(identifier: string | undefined): { prNumber: number | null; branchName: string | null } {
  if (!identifier) return { prNumber: null, branchName: null };
  const asNumber = Number(identifier);
  if (Number.isInteger(asNumber) && asNumber > 0 && String(asNumber) === identifier) {
    return { prNumber: asNumber, branchName: null };
  }
  return { prNumber: null, branchName: identifier };
}

// ── CI Check helpers ──────────────────────────────────────────────────────────

function formatCheckDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

const CHECK_STATUS_ICON: Record<string, typeof CheckCircle2> = {
  passed: CheckCircle2,
  failed: XCircle,
  skipped: CircleDot,
  timed_out: Clock,
  error: AlertTriangle,
};

const CHECK_STATUS_COLOR: Record<string, string> = {
  passed: 'text-green-400',
  failed: 'text-red-400',
  skipped: 'text-text-muted',
  timed_out: 'text-amber-400',
  error: 'text-red-400',
};

function CheckResultRow({ result }: { result: PrCheckResult }) {
  const Icon = CHECK_STATUS_ICON[result.status] ?? CircleDot;
  const color = CHECK_STATUS_COLOR[result.status] ?? 'text-text-muted';

  return (
    <div className="flex items-center gap-3 px-4 py-1.5 text-xs hover:bg-surface/30 transition-colors">
      <Icon size={13} className={`flex-shrink-0 ${color}`} />
      <span className="min-w-0 flex-1 truncate text-text-primary">{result.display_name}</span>
      <span className="text-[10px] text-text-muted">{result.category}</span>
      {result.docker_used && (
        <span title="Ran in Docker"><Container size={11} className="flex-shrink-0 text-blue-400" /></span>
      )}
      <span className="flex-shrink-0 font-mono text-[10px] text-text-muted">
        {formatCheckDuration(result.duration_ms)}
      </span>
    </div>
  );
}

function CiChecksPanel({
  checks,
  onRerun,
  isRerunning,
}: {
  checks: PrChecks | null | undefined;
  onRerun: () => void;
  isRerunning: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  if (!checks) {
    return (
      <div className="flex-shrink-0 border-b border-border bg-navy-800/20 px-6 py-2.5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 text-xs text-text-muted">
            <CircleDot size={13} />
            <span>No checks configured</span>
          </div>
          <button
            onClick={onRerun}
            disabled={isRerunning}
            className="flex items-center gap-1.5 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
          >
            {isRerunning ? <Loader2 size={11} className="animate-spin" /> : <RefreshCw size={11} />}
            Run Checks
          </button>
        </div>
      </div>
    );
  }

  const statusConfig = {
    passing: { icon: CheckCircle2, color: 'text-green-400', bg: 'bg-green-500/10 border-green-500/30', label: 'Passing' },
    failing: { icon: XCircle, color: 'text-red-400', bg: 'bg-red-500/10 border-red-500/30', label: 'Failing' },
    pending: { icon: Loader2, color: 'text-text-muted', bg: 'bg-navy-700/50 border-border', label: 'Pending' },
  } as const;

  const cfg = statusConfig[checks.status];
  const StatusIcon = cfg.icon;
  const passedCount = checks.results.filter((r) => r.status === 'passed').length;
  const failedCount = checks.results.filter((r) => r.status === 'failed').length;

  return (
    <div className={`flex-shrink-0 border-b border-border bg-navy-800/20`}>
      <div className="flex items-center justify-between px-6 py-2.5">
        <button
          onClick={() => setExpanded((v) => !v)}
          className="flex items-center gap-2 text-xs"
        >
          {expanded ? <ChevronDown size={12} className="text-text-muted" /> : <ChevronRight size={12} className="text-text-muted" />}
          <StatusIcon size={13} className={`${cfg.color} ${checks.status === 'pending' ? 'animate-spin' : ''}`} />
          <span className={`font-semibold ${cfg.color}`}>{cfg.label}</span>
          <span className="text-text-muted">
            {passedCount}/{checks.results.length} passed
            {failedCount > 0 && <span className="text-red-400 ml-1">({failedCount} failed)</span>}
          </span>
        </button>
        <button
          onClick={onRerun}
          disabled={isRerunning}
          className="flex items-center gap-1.5 rounded border border-border bg-surface px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
        >
          {isRerunning ? <Loader2 size={11} className="animate-spin" /> : <RefreshCw size={11} />}
          Re-run
        </button>
      </div>
      {expanded && checks.results.length > 0 && (
        <div className="border-t border-border/50 divide-y divide-border/30">
          {checks.results.map((result) => (
            <CheckResultRow key={result.name} result={result} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Branch Protection Banner ──────────────────────────────────────────────────

interface BranchProtectionBannerProps {
  protection: BranchProtectionInfo;
  approvalCount: number;
  ciPassing: boolean;
}

function BranchProtectionBanner({ protection, approvalCount, ciPassing }: BranchProtectionBannerProps) {
  const approvalsRequired = protection.required_approvals;
  const approvalsOk = approvalsRequired === 0 || approvalCount >= approvalsRequired;
  const ciOk = !protection.require_ci_pass || ciPassing;

  return (
    <div className="flex-shrink-0 border-b border-border bg-navy-800/20 px-6 py-2.5">
      <div className="flex items-center gap-2 mb-1.5">
        <ShieldCheck size={12} className="flex-shrink-0 text-text-muted" />
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Branch Protection</span>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        {approvalsRequired > 0 && (
          <div className={`flex items-center gap-1.5 rounded border px-2 py-1 text-xs ${
            approvalsOk
              ? 'border-green-500/30 bg-green-500/10'
              : 'border-amber-500/30 bg-amber-500/10'
          }`}>
            {approvalsOk
              ? <CheckCircle2 size={12} className="flex-shrink-0 text-green-400" />
              : <AlertTriangle size={12} className="flex-shrink-0 text-amber-400" />
            }
            <span className={approvalsOk ? 'text-green-400' : 'text-amber-400'}>
              {approvalCount} of {approvalsRequired} approval{approvalsRequired !== 1 ? 's' : ''} required
            </span>
          </div>
        )}
        {protection.require_ci_pass && (
          <div className={`flex items-center gap-1.5 rounded border px-2 py-1 text-xs ${
            ciOk
              ? 'border-green-500/30 bg-green-500/10'
              : 'border-red-500/30 bg-red-500/10'
          }`}>
            {ciOk
              ? <CheckCircle2 size={12} className="flex-shrink-0 text-green-400" />
              : <XCircle size={12} className="flex-shrink-0 text-red-400" />
            }
            <span className={ciOk ? 'text-green-400' : 'text-red-400'}>CI must pass</span>
          </div>
        )}
      </div>
    </div>
  );
}

/** Full PR detail view when identifier is a PR number */
function PrDetailView({ repoId, prNumber }: { repoId: string; prNumber: number }) {
  const navigate = useNavigate();
  const toast = useToast();

  const { data: pr, isLoading, error } = useGetPullRequest(repoId, prNumber);
  const updatePr = useUpdatePullRequest(repoId);
  const mergePr = useMergePullRequest(repoId);
  const runChecks = useRunPrChecks(repoId);
  const { data: branchProtectionList } = useListBranchProtection(repoId);

  // Fetch diff via branch comparison for displaying files changed
  const prView = usePullRequestView(
    repoId,
    pr?.state !== 'merged' ? pr?.source_branch : undefined,
  );

  // LLM AI Review
  const { data: llmConfig } = useLlmConfig(repoId);
  const llmReviewEnabled = (!!llmConfig?.server_enabled || !!llmConfig?.base_url) && (llmConfig?.enabled_features?.pr_review ?? true);
  const aiReviewStreamFn = useCallback(
    (signal: AbortSignal) => streamPrReview(repoId, prNumber, signal),
    [repoId, prNumber],
  );
  const aiReview = useLlmStream(aiReviewStreamFn);

  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState('');
  const [editingDescription, setEditingDescription] = useState(false);
  const [descDraft, setDescDraft] = useState('');
  const [prevPrId, setPrevPrId] = useState<number | null>(null);
  const [mergeStrategy, setMergeStrategy] = useState<PrMergeStrategy>('merge');
  const [showStrategyDropdown, setShowStrategyDropdown] = useState(false);
  const [confirmForce, setConfirmForce] = useState(false);
  const strategyRef = useRef<HTMLDivElement>(null);

  if (pr && pr.number !== prevPrId) {
    setPrevPrId(pr.number);
    setTitleDraft(pr.title);
    setDescDraft(pr.description);
  }

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (strategyRef.current && !strategyRef.current.contains(e.target as Node)) {
        setShowStrategyDropdown(false);
      }
    }
    if (showStrategyDropdown) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [showStrategyDropdown]);

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <LoadingSpinner size={24} />
      </div>
    );
  }

  if (error || !pr) {
    const message = axios.isAxiosError(error) && error.message ? error.message : 'Failed to load PR';
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
        <p className="text-sm font-medium text-status-deleted">{message}</p>
        <button onClick={() => navigate(-1)} className="text-xs text-text-muted hover:text-text-primary">
          Go back
        </button>
      </div>
    );
  }

  function handleSaveTitle() {
    if (!pr || titleDraft.trim() === pr.title) {
      setEditingTitle(false);
      return;
    }
    updatePr.mutate(
      { number: prNumber, payload: { title: titleDraft.trim() } },
      {
        onSuccess: () => {
          setEditingTitle(false);
          toast.success('Title updated');
        },
        onError: () => toast.error('Failed to update title'),
      },
    );
  }

  function handleSaveDescription() {
    if (!pr) return;
    updatePr.mutate(
      { number: prNumber, payload: { description: descDraft } },
      {
        onSuccess: () => {
          setEditingDescription(false);
          toast.success('Description updated');
        },
        onError: () => toast.error('Failed to update description'),
      },
    );
  }

  function handleToggleState() {
    if (!pr) return;
    const newState = pr.state === 'open' ? 'closed' : 'open';
    updatePr.mutate(
      { number: prNumber, payload: { state: newState } },
      {
        onSuccess: () => toast.success(`PR ${newState === 'closed' ? 'closed' : 'reopened'}`),
        onError: () => toast.error('Failed to update PR state'),
      },
    );
  }

  const checksFailing = pr?.checks?.status === 'failing';
  const ciPassing = pr?.checks?.status === 'passing';

  const activeProtection = branchProtectionList?.find((p) => p.branch === pr?.target_branch) ?? null;
  const approvalCount = pr?.reviews?.filter((r) => r.state === 'approved').length ?? 0;
  const approvalsOk = !activeProtection || activeProtection.required_approvals === 0 || approvalCount >= activeProtection.required_approvals;
  const ciProtectionOk = !activeProtection || !activeProtection.require_ci_pass || ciPassing;
  const protectionBlocked = activeProtection !== null && (!approvalsOk || !ciProtectionOk);

  function handleRunChecks() {
    runChecks.mutate(prNumber, {
      onSuccess: () => toast.success('Checks re-run complete'),
      onError: (err) => {
        const message = axios.isAxiosError(err) && err.message ? err.message : 'Failed to run checks';
        toast.error(message);
      },
    });
  }

  function handleMerge(strategy?: PrMergeStrategy, force?: boolean) {
    // Hard block: branch protection rules take precedence — no force path
    if (protectionBlocked) return;
    // If checks are failing (no protection rule) and force not confirmed, show confirmation
    if (checksFailing && !force && !confirmForce) {
      setConfirmForce(true);
      return;
    }
    setConfirmForce(false);
    mergePr.mutate(
      { number: prNumber, strategy: strategy ?? mergeStrategy, force: force ?? checksFailing },
      {
        onSuccess: (result) => {
          toast.success(`PR merged${result.merge_commit ? ` (${result.merge_commit.slice(0, 8)})` : ''}`);
        },
        onError: (err) => {
          const message = axios.isAxiosError(err) && err.message ? err.message : 'Merge failed';
          toast.error(message);
        },
      },
    );
  }

  const selectedLabel = MERGE_STRATEGIES.find((s) => s.value === mergeStrategy)?.label ?? 'Merge';

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="flex-shrink-0 border-b border-border bg-navy-900 px-6 py-4">
        {/* Title */}
        <div className="flex items-start gap-2">
          <GitPullRequest size={18} className={`mt-0.5 flex-shrink-0 ${
            pr.state === 'open' ? 'text-green-400' : pr.state === 'merged' ? 'text-purple-400' : 'text-red-400'
          }`} />
          {editingTitle ? (
            <div className="flex flex-1 items-center gap-2">
              <input
                value={titleDraft}
                onChange={(e) => setTitleDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleSaveTitle();
                  if (e.key === 'Escape') { setEditingTitle(false); setTitleDraft(pr.title); }
                }}
                className="flex-1 rounded border border-border bg-navy-950 px-2 py-1 text-sm text-text-primary focus:border-accent focus:outline-none"
                autoFocus
              />
              <button onClick={handleSaveTitle} className="rounded p-1 text-green-400 hover:bg-green-500/10"><Check size={14} /></button>
              <button onClick={() => { setEditingTitle(false); setTitleDraft(pr.title); }} className="rounded p-1 text-text-muted hover:bg-surface-hover"><X size={14} /></button>
            </div>
          ) : (
            <div className="flex flex-1 items-center gap-2">
              <h1 className="text-sm font-semibold text-text-primary">{pr.title}</h1>
              <span className="font-mono text-xs text-text-muted">#{pr.number}</span>
              <span className={`rounded px-1.5 py-0.5 text-[10px] font-bold uppercase ${STATE_BADGE_CLASS[pr.state] ?? ''}`}>
                {pr.state}
              </span>
              {pr.state !== 'merged' && (
                <button onClick={() => setEditingTitle(true)} className="rounded p-1 text-text-muted hover:bg-surface-hover hover:text-text-primary">
                  <Pencil size={12} />
                </button>
              )}
            </div>
          )}
        </div>

        {/* Branch info + diff stats */}
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-muted">
          <span className="font-mono text-text-secondary">{pr.source_branch}</span>
          <ArrowRight size={12} />
          <span className="font-mono text-text-secondary">{pr.target_branch}</span>
          <span>by {pr.author}</span>
          {prView.data && (
            <>
              <span className="text-border">|</span>
              <span className="text-[11px] font-semibold text-green-400" title="Additions">
                +{prView.data.diff.stats.additions.toLocaleString()}
              </span>
              <span className="text-[11px] font-semibold text-status-deleted" title="Deletions">
                -{prView.data.diff.stats.deletions.toLocaleString()}
              </span>
              <span className="text-[11px] text-text-muted" title="Files changed">
                {prView.data.diff.stats.files_changed} file{prView.data.diff.stats.files_changed !== 1 ? 's' : ''} changed
              </span>
            </>
          )}
        </div>

        {/* Actions */}
        <div className="mt-3 flex items-center gap-2 flex-wrap">
          {pr.state === 'open' && (
            <>
              {/* Protection-blocked warning banner */}
              {protectionBlocked && (
                <div className="flex w-full items-center gap-2 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 mb-1">
                  <ShieldCheck size={13} className="flex-shrink-0 text-amber-400" />
                  <span className="text-xs text-amber-400">
                    Merge blocked by branch protection:{' '}
                    {!approvalsOk && `${approvalCount} of ${activeProtection!.required_approvals} required approval${activeProtection!.required_approvals !== 1 ? 's' : ''}`}
                    {!approvalsOk && !ciProtectionOk && ' and '}
                    {!ciProtectionOk && 'CI must pass'}
                    .
                  </span>
                </div>
              )}

              {/* Checks-failing warning banner (only when no protection rule covers it) */}
              {checksFailing && !protectionBlocked && (
                <div className="flex w-full items-center gap-2 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 mb-1">
                  <AlertTriangle size={13} className="flex-shrink-0 text-amber-400" />
                  <span className="text-xs text-amber-400">CI checks are failing. Merge is blocked unless forced.</span>
                </div>
              )}

              {/* Force merge confirmation (only when no protection rule blocks it) */}
              {confirmForce && !protectionBlocked && (
                <div className="flex w-full items-center gap-2 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 mb-1">
                  <AlertTriangle size={13} className="flex-shrink-0 text-amber-400" />
                  <span className="text-xs text-amber-300">Checks are failing. Force merge anyway?</span>
                  <button
                    onClick={() => handleMerge(undefined, true)}
                    disabled={mergePr.isPending}
                    className="ml-auto flex items-center gap-1 rounded bg-amber-500 px-2 py-0.5 text-[11px] font-semibold text-navy-950 hover:bg-amber-400 disabled:opacity-50"
                  >
                    {mergePr.isPending ? <Loader2 size={11} className="animate-spin" /> : <GitMerge size={11} />}
                    Force Merge
                  </button>
                  <button
                    onClick={() => setConfirmForce(false)}
                    className="rounded px-2 py-0.5 text-[11px] text-text-muted hover:text-text-primary"
                  >
                    Cancel
                  </button>
                </div>
              )}

              {/* Merge split button */}
              <div className="relative" ref={strategyRef}>
                <div className="flex">
                  <button
                    onClick={() => handleMerge()}
                    disabled={mergePr.isPending || protectionBlocked}
                    className={`flex items-center gap-1.5 rounded-l-md px-3 py-1.5 text-xs font-semibold transition-colors disabled:opacity-50 ${
                      protectionBlocked
                        ? 'bg-surface text-text-muted cursor-not-allowed'
                        : checksFailing
                          ? 'bg-amber-500 text-navy-950 hover:bg-amber-400'
                          : 'bg-accent text-navy-950 hover:bg-accent-light'
                    }`}
                  >
                    {mergePr.isPending ? <Loader2 size={13} className="animate-spin" /> : <GitMerge size={13} />}
                    {mergePr.isPending ? 'Merging...' : checksFailing && !protectionBlocked ? `${selectedLabel} (force)` : selectedLabel}
                  </button>
                  <button
                    onClick={() => setShowStrategyDropdown((v) => !v)}
                    disabled={mergePr.isPending || protectionBlocked}
                    className={`flex items-center rounded-r-md border-l border-navy-950/20 px-1.5 py-1.5 text-navy-950 transition-colors disabled:opacity-50 ${
                      protectionBlocked
                        ? 'bg-surface border-border text-text-muted cursor-not-allowed'
                        : checksFailing
                          ? 'bg-amber-500 hover:bg-amber-400'
                          : 'bg-accent hover:bg-accent-light'
                    }`}
                    aria-label="Select merge strategy"
                  >
                    <ChevronDown size={13} />
                  </button>
                </div>
                {showStrategyDropdown && (
                  <div className="absolute left-0 top-full z-30 mt-1 w-52 overflow-hidden rounded-md border border-border bg-navy-800 shadow-lg">
                    {MERGE_STRATEGIES.map((s) => (
                      <button
                        key={s.value}
                        onClick={() => { setMergeStrategy(s.value); setShowStrategyDropdown(false); }}
                        className={`flex w-full px-3 py-2 text-left text-xs transition-colors hover:bg-surface-hover ${
                          mergeStrategy === s.value ? 'bg-accent/10 text-accent' : 'text-text-primary'
                        }`}
                      >
                        {s.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <button
                onClick={handleToggleState}
                disabled={updatePr.isPending}
                className="flex items-center gap-1.5 rounded-md border border-red-500/30 px-3 py-1.5 text-xs font-medium text-red-400 transition-colors hover:bg-red-500/10 disabled:opacity-50"
              >
                <X size={13} />
                Close PR
              </button>
            </>
          )}

          {pr.state === 'closed' && (
            <button
              onClick={handleToggleState}
              disabled={updatePr.isPending}
              className="flex items-center gap-1.5 rounded-md border border-green-500/30 px-3 py-1.5 text-xs font-medium text-green-400 transition-colors hover:bg-green-500/10 disabled:opacity-50"
            >
              <GitPullRequest size={13} />
              Reopen PR
            </button>
          )}

          {pr.state === 'merged' && pr.merge_commit && (
            <span className="flex items-center gap-1.5 text-xs text-purple-400">
              <GitMerge size={13} />
              Merged at commit <span className="font-mono">{pr.merge_commit.slice(0, 8)}</span>
            </span>
          )}
        </div>
      </div>

      {/* Description */}
      <div className="flex-shrink-0 border-b border-border bg-navy-800/30 px-6 py-3">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Description</span>
          {pr.state !== 'merged' && !editingDescription && (
            <button
              onClick={() => setEditingDescription(true)}
              className="rounded p-1 text-text-muted hover:bg-surface-hover hover:text-text-primary"
            >
              <Pencil size={11} />
            </button>
          )}
        </div>
        {editingDescription ? (
          <div className="mt-2 space-y-2">
            <textarea
              value={descDraft}
              onChange={(e) => setDescDraft(e.target.value)}
              rows={4}
              className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
            <div className="flex gap-2">
              <button onClick={handleSaveDescription} disabled={updatePr.isPending} className="rounded bg-accent px-3 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50">
                Save
              </button>
              <button onClick={() => { setEditingDescription(false); setDescDraft(pr.description); }} className="rounded px-3 py-1 text-xs text-text-muted hover:text-text-primary">
                Cancel
              </button>
            </div>
          </div>
        ) : pr.description ? (
          <div
            className="prose prose-invert max-w-none mt-1 text-xs"
            dangerouslySetInnerHTML={{
              __html: DOMPurify.sanitize(marked.parse(pr.description) as string),
            }}
          />
        ) : (
          <p className="mt-1 text-xs italic text-text-muted">No description provided.</p>
        )}
      </div>

      {/* CI Checks */}
      {pr.state === 'open' && (
        <CiChecksPanel
          checks={pr.checks}
          onRerun={handleRunChecks}
          isRerunning={runChecks.isPending}
        />
      )}

      {/* Branch Protection Status */}
      {pr.state === 'open' && activeProtection !== null && (
        <BranchProtectionBanner
          protection={activeProtection}
          approvalCount={approvalCount}
          ciPassing={ciPassing}
        />
      )}

      {/* AI Review */}
      {llmReviewEnabled && (
        <div className="px-4 pt-2">
          <LlmPanel
            title="AI Code Review"
            content={aiReview.content}
            isStreaming={aiReview.isStreaming}
            error={aiReview.error}
            onGenerate={aiReview.start}
            onCancel={aiReview.cancel}
          />
        </div>
      )}

      {/* Reviews & Comments */}
      <ReviewPanel repoId={repoId} prNumber={prNumber} prState={pr.state} />

      {/* Diff view — reuse PullRequestView component for the branch comparison */}
      <div className="min-h-0 flex-1 overflow-hidden">
        {prView.isLoading && (
          <div className="flex h-full items-center justify-center"><LoadingSpinner size={20} /></div>
        )}
        {prView.data && (
          <PullRequestViewComponent
            data={prView.data}
            repoId={repoId}
            onMerge={(strategy) => handleMerge(strategy)}
            isMerging={mergePr.isPending}
          />
        )}
        {!prView.isLoading && !prView.data && pr.state === 'merged' && (
          <div className="flex h-full items-center justify-center text-xs text-text-muted">
            Diff not available for merged PRs (source branch may have been deleted).
          </div>
        )}
        {!prView.isLoading && !prView.data && pr.state !== 'merged' && (
          <div className="flex h-full items-center justify-center text-xs text-status-deleted">
            {prView.error
              ? `Failed to load diff: ${prView.error instanceof Error ? prView.error.message : 'Unknown error'}`
              : 'No diff data available'}
          </div>
        )}
      </div>
    </div>
  );
}

/** Legacy branch comparison view */
function BranchCompareView({ repoId, branch }: { repoId: string; branch: string }) {
  const navigate = useNavigate();
  const toast = useToast();

  const { data, isLoading, error } = usePullRequestView(repoId, branch);
  const merge = useMergeBranch(repoId);

  function handleMerge(strategy: MergeStrategy) {
    if (!data) return;
    merge.mutate(
      { target: data.base, source: data.branch, strategy },
      {
        onSuccess: () => {
          toast.success(`Merged "${data.branch}" into "${data.base}"`);
          navigate(`/repo/${repoId}/pulls`);
        },
        onError: (err) => {
          const message = axios.isAxiosError(err) && err.message ? err.message : 'Merge failed';
          toast.error(message);
        },
      },
    );
  }

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center"><LoadingSpinner size={24} /></div>
    );
  }

  if (error || !data) {
    const message = axios.isAxiosError(error) && error.message ? error.message : 'Failed to load comparison';
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
        <p className="text-sm font-medium text-status-deleted">{message}</p>
        <button onClick={() => navigate(-1)} className="text-xs text-text-muted hover:text-text-primary">Go back</button>
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-hidden">
      <PullRequestViewComponent data={data} repoId={repoId} onMerge={handleMerge} isMerging={merge.isPending} />
    </div>
  );
}

function PullRequestPage() {
  const { repoId, '*': identifier } = useParams<{ repoId: string; '*': string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 PR \u2014 OVC`);
  const navigate = useNavigate();
  const { prNumber, branchName } = parsePrIdentifier(identifier);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Back nav strip */}
      <div className="flex-shrink-0 border-b border-border bg-navy-900 px-4 py-2">
        <button
          onClick={() => navigate(`/repo/${repoId}/pulls`)}
          className="flex items-center gap-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
        >
          <ArrowLeft size={13} />
          Pull Requests
        </button>
      </div>

      {prNumber !== null && repoId ? (
        <PrDetailView repoId={repoId} prNumber={prNumber} />
      ) : branchName !== null && repoId ? (
        <BranchCompareView repoId={repoId} branch={branchName} />
      ) : (
        <div className="flex h-full items-center justify-center text-sm text-text-muted">
          Invalid pull request identifier.
        </div>
      )}
    </div>
  );
}

export default PullRequestPage;
