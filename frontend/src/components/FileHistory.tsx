import { useNavigate } from 'react-router-dom';
import { GitCommitVertical, User, Clock, X, History } from 'lucide-react';
import { useFileHistory } from '../hooks/useRepo.ts';
import LoadingSpinner from './LoadingSpinner.tsx';

interface FileHistoryProps {
  repoId: string;
  filePath: string;
  onClose: () => void;
}

function FileHistory({ repoId, filePath, onClose }: FileHistoryProps) {
  const navigate = useNavigate();
  const { data, isLoading, error } = useFileHistory(repoId, filePath);

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-shrink-0 items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <History size={15} className="text-accent" />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-text-secondary">
          {filePath}
        </span>
        <button
          onClick={onClose}
          className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
          aria-label="Close file history"
        >
          <X size={14} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto">
        {isLoading && (
          <LoadingSpinner className="h-32" message="Loading file history..." />
        )}

        {error && (
          <p className="p-4 text-xs text-status-deleted">
            Failed to load file history: {error.message}
          </p>
        )}

        {data && data.commits.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-2 py-12 text-text-muted">
            <GitCommitVertical size={28} />
            <p className="text-xs">No commits found for this file</p>
          </div>
        )}

        {data && data.commits.length > 0 && (
          <div className="divide-y divide-border">
            {data.commits.map((commit) => (
              <button
                key={commit.id}
                onClick={() =>
                  navigate(`/repo/${repoId}/history?commit=${commit.id}`)
                }
                className="w-full px-4 py-3 text-left transition-colors hover:bg-surface-hover"
              >
                <div className="flex items-start gap-2">
                  <GitCommitVertical
                    size={14}
                    className="mt-0.5 flex-shrink-0 text-accent/60"
                  />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-xs font-medium text-text-primary">
                      {commit.message.split('\n')[0]}
                    </p>
                    <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5">
                      <span className="flex items-center gap-1 text-[11px] text-text-muted">
                        <User size={10} />
                        {commit.author.name}
                      </span>
                      <span className="flex items-center gap-1 text-[11px] text-text-muted">
                        <Clock size={10} />
                        {new Date(commit.authored_at).toLocaleDateString()}
                      </span>
                      <span className="font-mono text-[11px] text-accent/70">
                        {commit.short_id}
                      </span>
                    </div>
                  </div>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default FileHistory;
