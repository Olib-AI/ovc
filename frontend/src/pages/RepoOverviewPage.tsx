import { useParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import {
  useRepo,
  useCommitLog,
  useBranches,
  useTags,
  useRepoStatus,
} from '../hooks/useRepo.ts';
import { SkeletonBlock } from '../components/Skeleton.tsx';
import {
  GitBranch,
  GitCommitVertical,
  Tag,
  FileCode,
  Lock,
  Unlock,
  GitPullRequest,
  Zap,
  Plus,
  Clock,
  ShieldCheck,
} from 'lucide-react';

function formatRelativeTime(iso: string): string {
  const now = Date.now();
  const diffMs = now - new Date(iso).getTime();
  const diffSec = Math.floor(diffMs / 1000);

  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays < 30) return `${diffDays}d ago`;
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function RepoOverviewPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Overview \u2014 OVC`);

  const { data: repo, isLoading: repoLoading } = useRepo(repoId);
  const { data: log } = useCommitLog(repoId, 5);
  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);
  const { data: status } = useRepoStatus(repoId);

  if (repoLoading) {
    return (
      <div className="h-full overflow-y-auto p-6">
        <div className="mb-6 flex items-start justify-between">
          <div className="space-y-2">
            <SkeletonBlock className="h-7 w-48" />
            <SkeletonBlock className="h-4 w-32" />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 mb-6">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="rounded-lg border border-border bg-navy-900 p-4">
              <SkeletonBlock className="mb-2 h-3 w-16" />
              <SkeletonBlock className="h-6 w-12" />
            </div>
          ))}
        </div>
        <div className="space-y-3">
          {Array.from({ length: 5 }).map((_, i) => (
            <div key={i} className="flex items-center gap-3 rounded-lg border border-border bg-navy-900 p-3">
              <SkeletonBlock className="h-4 w-4 flex-shrink-0 rounded-full" />
              <div className="flex-1 space-y-1.5">
                <SkeletonBlock className={`h-4 ${i % 2 === 0 ? 'w-3/4' : 'w-2/3'}`} />
                <SkeletonBlock className="h-3 w-32" />
              </div>
              <SkeletonBlock className="h-3 w-16 flex-shrink-0" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (!repo) return null;

  const recentCommits = log?.commits.slice(0, 5) ?? [];
  const currentBranch = status?.branch ?? 'unknown';
  const latestCommit = recentCommits[0];
  const isEncrypted = true; // OVC repos are always encrypted

  const stats = repo.repo_stats;

  return (
    <div className="h-full overflow-y-auto">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <GitBranch size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Overview</h1>
        <span className="font-mono text-xs text-text-muted">{repo.name}</span>
      </div>

      <div className="mx-auto max-w-2xl space-y-4 p-5">
        {/* Repo identity card */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-accent/15 text-lg font-bold text-accent">
              {repo.name.slice(0, 2).toUpperCase()}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <h2 className="text-base font-bold text-text-primary">{repo.name}</h2>
                <span
                  className={`flex items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] font-semibold ${
                    isEncrypted
                      ? 'border-accent/30 bg-accent/10 text-accent'
                      : 'border-border bg-surface text-text-muted'
                  }`}
                  title="Repository encryption status"
                >
                  {isEncrypted ? <Lock size={9} /> : <Unlock size={9} />}
                  {isEncrypted ? 'Encrypted' : 'Plain'}
                </span>
              </div>
              <p className="mt-0.5 truncate font-mono text-[11px] text-text-muted" title={repo.path}>
                {repo.path}
              </p>
            </div>
          </div>

          {/* Current branch + latest commit */}
          {latestCommit && (
            <div className="mt-3 flex items-center gap-2 rounded border border-border bg-navy-950 px-3 py-2">
              <GitBranch size={13} className="flex-shrink-0 text-accent" />
              <span className="font-mono text-xs font-semibold text-accent">{currentBranch}</span>
              <span className="text-xs text-text-muted">·</span>
              <span className="font-mono text-[11px] text-text-muted">{latestCommit.short_id}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-text-secondary" title={latestCommit.message}>
                {latestCommit.message}
              </span>
              <span className="flex-shrink-0 text-[11px] text-text-muted">
                {formatRelativeTime(latestCommit.authored_at)}
              </span>
            </div>
          )}
        </div>

        {/* Stats grid */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <StatCard
            icon={GitCommitVertical}
            value={stats.total_commits}
            label="Commits"
            color="text-accent"
          />
          <StatCard
            icon={GitBranch}
            value={stats.total_branches}
            label="Branches"
            color="text-blue-400"
          />
          <StatCard
            icon={Tag}
            value={stats.total_tags}
            label="Tags"
            color="text-purple-400"
          />
          <StatCard
            icon={FileCode}
            value={stats.tracked_files}
            label="Files"
            color="text-green-400"
          />
        </div>

        {/* Recent activity */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center gap-2">
            <Clock size={14} className="text-accent" />
            <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
              Recent Activity
            </h3>
          </div>

          {recentCommits.length === 0 ? (
            <p className="text-xs text-text-muted">No commits yet</p>
          ) : (
            <div className="space-y-2">
              {recentCommits.map((commit) => (
                <a
                  key={commit.id}
                  href={`/repo/${repoId}/history?commit=${commit.id}`}
                  className="flex items-start gap-2.5 rounded-md px-2 py-2 transition-colors hover:bg-surface-hover"
                >
                  <GitCommitVertical size={13} className="mt-0.5 flex-shrink-0 text-text-muted" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-xs text-text-primary">{commit.message}</p>
                    <p className="mt-0.5 text-[11px] text-text-muted">
                      {commit.author.name}
                    </p>
                  </div>
                  <div className="flex-shrink-0 text-right">
                    <p className="font-mono text-[11px] text-accent/80">{commit.short_id}</p>
                    <p className="text-[11px] text-text-muted">{formatRelativeTime(commit.authored_at)}</p>
                  </div>
                  {commit.signature_status === 'verified' && (
                    <span title="Verified signature"><ShieldCheck size={12} className="mt-0.5 flex-shrink-0 text-green-400" /></span>
                  )}
                </a>
              ))}
            </div>
          )}

          <div className="mt-3 border-t border-border pt-3">
            <a
              href={`/repo/${repoId}/history`}
              className="text-xs text-accent transition-colors hover:text-accent-light"
            >
              View full history &rarr;
            </a>
          </div>
        </div>

        {/* Quick actions */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-text-muted">
            Quick Actions
          </h3>
          <div className="flex flex-wrap gap-2">
            <QuickActionLink
              href={`/repo/${repoId}?view=changes`}
              icon={GitCommitVertical}
              label="New Commit"
            />
            <QuickActionLink
              href={`/repo/${repoId}/pulls`}
              icon={GitPullRequest}
              label="Pull Requests"
            />
            <QuickActionLink
              href={`/repo/${repoId}/actions`}
              icon={Zap}
              label="Run Actions"
            />
            <QuickActionLink
              href={`/repo/${repoId}/history`}
              icon={Clock}
              label="History"
            />
            <QuickActionLink
              href={`/repo/${repoId}/settings`}
              icon={Plus}
              label="Settings"
            />
          </div>
        </div>

        {/* Branches snapshot */}
        {branches && branches.length > 0 && (
          <div className="rounded-lg border border-border bg-navy-900 p-4">
            <div className="mb-3 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <GitBranch size={14} className="text-accent" />
                <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
                  Branches ({branches.length})
                </h3>
              </div>
              <a href={`/repo/${repoId}`} className="text-xs text-accent hover:text-accent-light">
                Manage &rarr;
              </a>
            </div>
            <div className="space-y-1">
              {branches.slice(0, 6).map((branch) => (
                <div
                  key={branch.name}
                  className="flex items-center gap-2 rounded px-2 py-1.5 hover:bg-surface-hover"
                >
                  <GitBranch size={12} className={`flex-shrink-0 ${branch.is_current ? 'text-accent' : 'text-text-muted'}`} />
                  <span className={`font-mono text-xs ${branch.is_current ? 'font-semibold text-accent' : 'text-text-secondary'}`}>
                    {branch.name}
                  </span>
                  {branch.is_current && (
                    <span className="ml-auto rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-semibold text-accent">
                      current
                    </span>
                  )}
                </div>
              ))}
              {branches.length > 6 && (
                <p className="px-2 pt-1 text-[11px] text-text-muted">
                  +{branches.length - 6} more branches
                </p>
              )}
            </div>
          </div>
        )}

        {/* Tags snapshot */}
        {tags && tags.length > 0 && (
          <div className="rounded-lg border border-border bg-navy-900 p-4">
            <div className="mb-3 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Tag size={14} className="text-accent" />
                <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
                  Tags ({tags.length})
                </h3>
              </div>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {tags.slice(0, 12).map((tag) => (
                <span
                  key={tag.name}
                  className="rounded border border-border bg-navy-950 px-2 py-0.5 font-mono text-[11px] text-text-secondary"
                >
                  {tag.name}
                </span>
              ))}
              {tags.length > 12 && (
                <span className="rounded border border-border bg-navy-950 px-2 py-0.5 text-[11px] text-text-muted">
                  +{tags.length - 12} more
                </span>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

interface StatCardProps {
  icon: typeof GitBranch;
  value: number;
  label: string;
  color: string;
}

function StatCard({ icon: Icon, value, label, color }: StatCardProps) {
  return (
    <div className="flex flex-col items-center gap-1 rounded-lg border border-border bg-navy-900 px-3 py-3">
      <Icon size={18} className={color} />
      <span className="text-lg font-bold text-text-primary">{value.toLocaleString()}</span>
      <span className="text-[11px] text-text-muted">{label}</span>
    </div>
  );
}

interface QuickActionLinkProps {
  href: string;
  icon: typeof GitBranch;
  label: string;
}

function QuickActionLink({ href, icon: Icon, label }: QuickActionLinkProps) {
  return (
    <a
      href={href}
      className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary"
    >
      <Icon size={13} className="flex-shrink-0 text-accent" />
      {label}
    </a>
  );
}

export default RepoOverviewPage;
