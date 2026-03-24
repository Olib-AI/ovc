import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { GitPullRequest, GitBranch, ArrowRight, Plus, X } from 'lucide-react';
import {
  useBranches,
  useRepoStatus,
  useListPullRequests,
  useCreatePullRequest,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import { TableRowsSkeleton } from '../components/Skeleton.tsx';
import type { PullRequestState } from '../api/types.ts';
import axios from 'axios';

const STATE_TABS: { label: string; value: PullRequestState | 'all' }[] = [
  { label: 'Open', value: 'open' },
  { label: 'Closed', value: 'closed' },
  { label: 'Merged', value: 'merged' },
];

const STATE_BADGE_CLASS: Record<PullRequestState, string> = {
  open: 'bg-green-500/15 text-green-400',
  closed: 'bg-red-500/15 text-red-400',
  merged: 'bg-purple-500/15 text-purple-400',
};

function timeAgo(dateStr: string): string {
  const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

function PullRequestsPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Pull Requests \u2014 OVC`);
  const navigate = useNavigate();
  const toast = useToast();

  const [activeTab, setActiveTab] = useState<PullRequestState | 'all'>('open');
  const [showCreate, setShowCreate] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [newDescription, setNewDescription] = useState('');
  const [sourceBranch, setSourceBranch] = useState('');
  const [targetBranch, setTargetBranch] = useState('');

  const { data: branches, isLoading: branchesLoading } = useBranches(repoId);
  const { data: status } = useRepoStatus(repoId);
  const { data: allPullRequests, isLoading: prsLoading } = useListPullRequests(repoId, 'all');
  const createPr = useCreatePullRequest(repoId ?? '');

  const currentBranch = status?.branch ?? branches?.find((b) => b.is_current)?.name;
  const comparableBranches = branches?.filter((b) => !b.is_current) ?? [];

  const prCounts: Record<PullRequestState, number> = { open: 0, closed: 0, merged: 0 };
  if (allPullRequests) {
    for (const pr of allPullRequests) {
      prCounts[pr.state]++;
    }
  }

  const pullRequests = allPullRequests?.filter(
    (pr) => activeTab === 'all' || pr.state === activeTab,
  );

  function handleCompare(branchName: string) {
    if (!repoId) return;
    navigate(`/repo/${repoId}/pulls/${encodeURIComponent(branchName)}`);
  }

  function handleCreatePR() {
    if (!repoId || !newTitle.trim() || !sourceBranch) return;
    createPr.mutate(
      {
        title: newTitle.trim(),
        description: newDescription.trim() || undefined,
        source_branch: sourceBranch,
        target_branch: targetBranch || undefined,
      },
      {
        onSuccess: (pr) => {
          toast.success(`Created PR #${pr.number}: ${pr.title}`);
          setShowCreate(false);
          setNewTitle('');
          setNewDescription('');
          setSourceBranch('');
          setTargetBranch('');
          navigate(`/repo/${repoId}/pulls/${pr.number}`);
        },
        onError: (err) => {
          const message = axios.isAxiosError(err) && err.message ? err.message : 'Failed to create PR';
          toast.error(message);
        },
      },
    );
  }

  if (branchesLoading || prsLoading) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex-shrink-0 border-b border-border bg-navy-900 px-6 py-4">
          <div className="flex items-center gap-2">
            <GitPullRequest size={18} className="text-accent" />
            <h1 className="text-sm font-semibold text-text-primary">Pull Requests</h1>
          </div>
        </div>
        <TableRowsSkeleton rows={8} cols={4} />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Page header */}
      <div className="flex-shrink-0 border-b border-border bg-navy-900 px-6 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <GitPullRequest size={18} className="text-accent" />
            <h1 className="text-sm font-semibold text-text-primary">Pull Requests</h1>
          </div>
          <button
            onClick={() => setShowCreate(!showCreate)}
            className="flex items-center gap-1.5 rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light"
          >
            {showCreate ? <X size={13} /> : <Plus size={13} />}
            {showCreate ? 'Cancel' : 'New Pull Request'}
          </button>
        </div>
      </div>

      {/* Create PR form */}
      {showCreate && (
        <div className="flex-shrink-0 border-b border-border bg-navy-800/30 px-6 py-4">
          <div className="space-y-3">
            <div className="flex gap-3">
              <div className="flex-1">
                <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                  Source Branch *
                </label>
                <select
                  value={sourceBranch}
                  onChange={(e) => setSourceBranch(e.target.value)}
                  className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary focus:border-accent focus:outline-none"
                >
                  <option value="">Select branch...</option>
                  {branches?.map((b) => (
                    <option key={b.name} value={b.name}>
                      {b.name}{b.is_current ? ' (current)' : ''}
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex items-end pb-0.5">
                <ArrowRight size={16} className="text-text-muted" />
              </div>
              <div className="flex-1">
                <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                  Target Branch
                </label>
                <select
                  value={targetBranch}
                  onChange={(e) => setTargetBranch(e.target.value)}
                  className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary focus:border-accent focus:outline-none"
                >
                  <option value="">{currentBranch ?? 'default'}</option>
                  {branches?.map((b) => (
                    <option key={b.name} value={b.name}>{b.name}</option>
                  ))}
                </select>
              </div>
            </div>
            <div>
              <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                Title *
              </label>
              <input
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleCreatePR()}
                placeholder="PR title..."
                className="w-full rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                Description
              </label>
              <textarea
                value={newDescription}
                onChange={(e) => setNewDescription(e.target.value)}
                placeholder="Describe your changes (markdown supported)..."
                rows={3}
                className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1.5 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              />
            </div>
            <button
              onClick={handleCreatePR}
              disabled={createPr.isPending || !newTitle.trim() || !sourceBranch}
              className="rounded-md bg-accent px-4 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
            >
              {createPr.isPending ? 'Creating...' : 'Create Pull Request'}
            </button>
          </div>
        </div>
      )}

      {/* Tabs */}
      <div className="flex-shrink-0 border-b border-border bg-navy-900/50 px-6">
        <div className="flex gap-0">
          {STATE_TABS.map(({ label, value }) => {
            const count = value === 'all' ? (pullRequests?.length ?? 0) : prCounts[value as PullRequestState];
            return (
              <button
                key={value}
                onClick={() => setActiveTab(value)}
                className={`relative px-4 py-2.5 text-xs font-medium transition-colors ${
                  activeTab === value
                    ? 'text-accent'
                    : 'text-text-muted hover:text-text-secondary'
                }`}
              >
                {label}
                {count > 0 && (
                  <span className={`ml-1.5 rounded-full px-1.5 py-0.5 text-[10px] font-semibold ${
                    activeTab === value
                      ? 'bg-accent/15 text-accent'
                      : 'bg-surface text-text-muted'
                  }`}>
                    {count}
                  </span>
                )}
                {activeTab === value && (
                  <span className="absolute bottom-0 left-0 right-0 h-0.5 bg-accent" />
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* PR list */}
      <div className="flex-1 overflow-y-auto px-6 py-4">
        {pullRequests && pullRequests.length > 0 ? (
          <ul className="space-y-2" role="list">
            {pullRequests.map((pr) => (
              <li key={pr.number}>
                <button
                  onClick={() => {
                    if (!repoId) return;
                    navigate(`/repo/${repoId}/pulls/${pr.number}`);
                  }}
                  className="group flex w-full items-center gap-3 rounded-lg border border-border bg-navy-800/50 px-4 py-3 text-left transition-colors hover:border-accent/40 hover:bg-navy-800"
                >
                  <GitPullRequest
                    size={15}
                    className={`flex-shrink-0 ${
                      pr.state === 'open' ? 'text-green-400' : pr.state === 'merged' ? 'text-purple-400' : 'text-red-400'
                    }`}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-medium text-text-primary">
                        {pr.title}
                      </span>
                      <span className={`flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase ${STATE_BADGE_CLASS[pr.state]}`}>
                        {pr.state}
                      </span>
                      {pr.checks?.status === 'passing' && (
                        <span className="flex-shrink-0 h-2 w-2 rounded-full bg-green-400" title="Checks passing" />
                      )}
                      {pr.checks?.status === 'failing' && (
                        <span className="flex-shrink-0 h-2 w-2 rounded-full bg-red-400" title="Checks failing" />
                      )}
                      {pr.checks?.status === 'pending' && (
                        <span className="flex-shrink-0 h-2 w-2 rounded-full bg-gray-400" title="Checks pending" />
                      )}
                    </div>
                    <div className="mt-1 flex items-center gap-2 text-[11px] text-text-muted">
                      <span className="font-mono">#{pr.number}</span>
                      <span className="flex items-center gap-1">
                        <span className="font-mono text-text-secondary">{pr.source_branch}</span>
                        <ArrowRight size={10} />
                        <span className="font-mono text-text-secondary">{pr.target_branch}</span>
                      </span>
                      <span>{pr.author}</span>
                      <span>{timeAgo(pr.updated_at)}</span>
                    </div>
                  </div>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <div className="flex flex-col items-center gap-3 py-16 text-center">
            <GitPullRequest size={32} className="text-text-muted/40" />
            <p className="text-sm text-text-muted">No {activeTab !== 'all' ? activeTab : ''} pull requests</p>
            <p className="text-xs text-text-muted/70">
              Create a pull request to start reviewing code changes.
            </p>
          </div>
        )}
      </div>

      {/* Quick Compare section */}
      <div className="flex-shrink-0 border-t border-border bg-navy-900/50 px-6 py-3">
        <details>
          <summary className="cursor-pointer text-[11px] font-semibold uppercase tracking-wider text-text-muted hover:text-text-secondary">
            Quick Compare (branch diff)
          </summary>
          <div className="mt-2 space-y-1">
            {comparableBranches.length === 0 ? (
              <p className="text-xs text-text-muted">No other branches to compare.</p>
            ) : (
              comparableBranches.map((branch) => (
                <button
                  key={branch.name}
                  onClick={() => handleCompare(branch.name)}
                  className="group flex w-full items-center gap-2 rounded px-2 py-1.5 text-left transition-colors hover:bg-surface-hover"
                >
                  <GitBranch size={12} className="flex-shrink-0 text-text-muted group-hover:text-accent" />
                  <span className="truncate font-mono text-xs text-text-secondary group-hover:text-text-primary">
                    {branch.name}
                  </span>
                  {currentBranch && (
                    <span className="flex flex-shrink-0 items-center gap-1 text-[10px] text-text-muted">
                      <ArrowRight size={9} />
                      {currentBranch}
                    </span>
                  )}
                </button>
              ))
            )}
          </div>
        </details>
      </div>
    </div>
  );
}

export default PullRequestsPage;
