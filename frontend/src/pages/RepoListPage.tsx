import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { GitBranch, FileCode, Tag, GitCommitVertical, Plus, RefreshCw } from 'lucide-react';
import { useRepos, useCreateRepo } from '../hooks/useRepo.ts';
import { useQueryClient } from '@tanstack/react-query';
import { RepoListSkeleton } from '../components/Skeleton.tsx';
import CreateRepoModal from '../components/CreateRepoModal.tsx';
import axios from 'axios';

function RepoListPage() {
  useDocumentTitle('Repositories \u2014 OVC');
  const { data: repos, isLoading, error } = useRepos();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const createRepo = useCreateRepo();

  function handleCreate(name: string, password: string) {
    setCreateError(null);
    createRepo.mutate(
      { name, password },
      {
        onSuccess: (repo) => {
          setShowCreateModal(false);
          navigate(`/repo/${repo.id}`);
        },
        onError: (err) => {
          if (axios.isAxiosError(err)) {
            setCreateError(
              (err.response?.data as { error?: string } | undefined)?.error ?? err.message,
            );
          } else {
            setCreateError('Failed to create repository');
          }
        },
      },
    );
  }

  if (isLoading) {
    return <RepoListSkeleton />;
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="w-full max-w-sm text-center">
          <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-status-deleted/10">
            <GitBranch size={20} className="text-status-deleted" />
          </div>
          <p className="mb-1 text-sm font-semibold text-text-primary">Failed to load repositories</p>
          <p className="mb-4 text-xs text-text-secondary">{error.message}</p>
          <button
            onClick={() => void queryClient.invalidateQueries({ queryKey: ['repos'] })}
            className="flex items-center gap-1.5 mx-auto rounded bg-accent px-4 py-2 text-sm font-medium text-navy-950 transition-colors hover:bg-accent-light"
          >
            <RefreshCw size={14} />
            Retry
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mb-6 flex items-start justify-between">
        <div>
          <h1 className="text-xl font-bold text-text-primary">Repositories</h1>
          <p className="mt-1 text-sm text-text-muted">
            Manage your OVC encrypted repositories
          </p>
        </div>
        <button
          onClick={() => setShowCreateModal(true)}
          className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light"
        >
          <Plus size={14} />
          New Repository
        </button>
      </div>

      {repos && repos.length === 0 && (
        <div className="flex flex-col items-center justify-center py-16 text-text-muted">
          <GitBranch size={48} className="mb-4 text-accent/30" />
          <p className="text-sm">No repositories found</p>
          <p className="mt-1 text-xs text-text-muted">
            Create a new repository to get started
          </p>
          <button
            onClick={() => setShowCreateModal(true)}
            className="mt-4 flex items-center gap-1.5 rounded bg-accent px-4 py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light"
          >
            <Plus size={14} />
            Create Repository
          </button>
        </div>
      )}

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {repos?.map((repo) => (
          <button
            key={repo.id}
            onClick={() => navigate(`/repo/${repo.id}`)}
            className="group rounded-lg border border-border bg-navy-900 p-4 text-left transition-all hover:border-accent/40 hover:bg-navy-800"
          >
            <div className="flex items-start justify-between">
              <div className="flex items-center gap-2">
                <div className="flex h-8 w-8 items-center justify-center rounded-md bg-accent/15 transition-colors group-hover:bg-accent/25">
                  <GitBranch size={16} className="text-accent" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-text-primary">{repo.name}</h3>
                  <p className="font-mono text-[10px] text-text-muted truncate max-w-[150px]">
                    {repo.head || 'empty'}
                  </p>
                </div>
              </div>
            </div>

            <div className="mt-3 flex items-center gap-3 text-xs text-text-muted">
              <span className="flex items-center gap-1">
                <GitCommitVertical size={12} />
                {repo.repo_stats.total_commits}
              </span>
              <span className="flex items-center gap-1">
                <GitBranch size={12} />
                {repo.repo_stats.total_branches}
              </span>
              <span className="flex items-center gap-1">
                <Tag size={12} />
                {repo.repo_stats.total_tags}
              </span>
              <span className="flex items-center gap-1">
                <FileCode size={12} />
                {repo.repo_stats.tracked_files}
              </span>
            </div>
          </button>
        ))}
      </div>

      {showCreateModal && (
        <CreateRepoModal
          onClose={() => {
            setShowCreateModal(false);
            setCreateError(null);
          }}
          onCreate={handleCreate}
          isCreating={createRepo.isPending}
          error={createError}
        />
      )}
    </div>
  );
}

export default RepoListPage;
