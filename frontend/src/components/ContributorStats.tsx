import { useState } from 'react';
import { Users, ChevronDown, ChevronRight } from 'lucide-react';
import { useShortlog } from '../hooks/useRepo.ts';

interface ContributorStatsProps {
  repoId: string;
}

function ContributorStats({ repoId }: ContributorStatsProps) {
  const [expanded, setExpanded] = useState(false);
  const { data, isLoading } = useShortlog(expanded ? repoId : undefined);

  const totalCommits = data?.authors.reduce((sum, a) => sum + a.count, 0) ?? 0;
  const maxCount = data?.authors[0]?.count ?? 1;

  return (
    <div className="border-b border-border">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-4 py-2 text-left transition-colors hover:bg-surface-hover/50"
        aria-expanded={expanded}
      >
        {expanded ? (
          <ChevronDown size={14} className="flex-shrink-0 text-text-muted" />
        ) : (
          <ChevronRight size={14} className="flex-shrink-0 text-text-muted" />
        )}
        <Users size={13} className="flex-shrink-0 text-accent" />
        <span className="text-xs font-medium text-text-secondary">Contributors</span>
        {data && (
          <span className="ml-auto text-[11px] text-text-muted">
            {data.authors.length} author{data.authors.length !== 1 ? 's' : ''} · {totalCommits} commit{totalCommits !== 1 ? 's' : ''}
          </span>
        )}
      </button>

      {expanded && (
        <div className="px-4 pb-3 pt-1">
          {isLoading && (
            <p className="text-xs text-text-muted">Loading contributors...</p>
          )}
          {data && data.authors.length === 0 && (
            <p className="text-xs text-text-muted">No contributors found</p>
          )}
          {data && data.authors.length > 0 && (
            <div className="space-y-1.5">
              {data.authors.map((author) => {
                const pct = Math.round((author.count / maxCount) * 100);
                const totalPct = totalCommits > 0
                  ? Math.round((author.count / totalCommits) * 100)
                  : 0;
                return (
                  <div key={`${author.name}:${author.email}`} className="group">
                    <div className="mb-0.5 flex items-baseline gap-2">
                      <span className="min-w-0 flex-1 truncate text-xs text-text-primary">
                        {author.name}
                      </span>
                      <span className="flex-shrink-0 text-[11px] tabular-nums text-text-muted">
                        {author.count} ({totalPct}%)
                      </span>
                    </div>
                    {/* CSS bar chart — width driven by percentage relative to top contributor */}
                    <div className="h-1 w-full overflow-hidden rounded-full bg-navy-700">
                      <div
                        className="h-full rounded-full bg-accent/70 transition-[width] duration-300"
                        style={{ width: `${pct}%` }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default ContributorStats;
