import { useState, useCallback } from 'react';
import { useParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { useQuery } from '@tanstack/react-query';
import { getBlame } from '../api/client.ts';
import { useBranches, useTags } from '../hooks/useRepo.ts';
import BlameView, { BlameViewSkeleton } from '../components/BlameView.tsx';
import { FileCode, ChevronRight, GitBranch, Tag, Hash } from 'lucide-react';

function BlamePage() {
  const { repoId, '*': filePath } = useParams<{ repoId: string; '*': string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Blame \u2014 OVC`);
  const [selectedRef, setSelectedRef] = useState<string>('');
  const [commitHashInput, setCommitHashInput] = useState('');
  const [showRefPicker, setShowRefPicker] = useState(false);

  const activeRef = selectedRef || undefined;

  const { data: blame, isLoading, error } = useQuery({
    queryKey: ['repo', repoId, 'blame', filePath, activeRef ?? ''],
    queryFn: () => getBlame(repoId!, filePath!, activeRef),
    enabled: !!repoId && !!filePath,
    gcTime: 30_000, // blame data is heavy — GC after 30s unmounted
  });

  const { data: branches } = useBranches(repoId);
  const { data: tags } = useTags(repoId);

  const handleSelectRef = useCallback((ref: string) => {
    setSelectedRef(ref);
    setShowRefPicker(false);
  }, []);

  const handleCommitHashSubmit = useCallback(() => {
    const trimmed = commitHashInput.trim();
    if (trimmed) {
      setSelectedRef(trimmed);
      setCommitHashInput('');
      setShowRefPicker(false);
    }
  }, [commitHashInput]);

  if (!filePath) {
    return (
      <div className="flex h-full items-center justify-center text-text-muted">
        <p className="text-sm">No file path specified</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-8 text-status-deleted">
        <p className="text-sm">Failed to load blame: {(error as Error).message}</p>
      </div>
    );
  }

  const pathSegments = filePath.split('/');
  // Render the header + skeleton while loading so the layout doesn't flash
  const showSkeleton = isLoading || !blame;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <FileCode size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Blame</h1>
        <ChevronRight size={12} className="text-text-muted" />
        <div className="flex items-center gap-1 font-mono text-xs text-text-secondary">
          {pathSegments.map((segment, idx) => (
            <span key={idx} className="flex items-center gap-1">
              {idx > 0 && <span className="text-text-muted">/</span>}
              <span className={idx === pathSegments.length - 1 ? 'text-text-primary' : ''}>
                {segment}
              </span>
            </span>
          ))}
        </div>

        {/* Ref Picker */}
        <div className="relative ml-auto flex items-center gap-2">
          <button
            onClick={() => setShowRefPicker(!showRefPicker)}
            className="flex items-center gap-1.5 rounded border border-border bg-navy-950 px-2.5 py-1 text-[11px] font-medium text-text-secondary transition-colors hover:border-accent hover:text-text-primary"
          >
            <GitBranch size={11} />
            {selectedRef ? (
              <span className="max-w-[140px] truncate font-mono">{selectedRef.length > 12 ? selectedRef.slice(0, 12) : selectedRef}</span>
            ) : (
              <span>HEAD</span>
            )}
          </button>
          {selectedRef && (
            <button
              onClick={() => setSelectedRef('')}
              className="rounded px-1.5 py-0.5 text-[10px] font-medium text-accent transition-colors hover:bg-accent/10"
            >
              Reset to HEAD
            </button>
          )}
          {blame && (
            <span className="text-xs text-text-muted">
              {blame.lines.length} line{blame.lines.length !== 1 ? 's' : ''}
            </span>
          )}

          {showRefPicker && (
            <div className="absolute right-0 top-full z-20 mt-1 w-72 rounded-lg border border-border bg-navy-800 shadow-xl">
              {/* Commit hash input */}
              <div className="border-b border-border p-2">
                <label className="mb-1 flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-text-muted">
                  <Hash size={10} />
                  Commit Hash
                </label>
                <div className="flex gap-1">
                  <input
                    value={commitHashInput}
                    onChange={(e) => setCommitHashInput(e.target.value)}
                    onKeyDown={(e) => { if (e.key === 'Enter') handleCommitHashSubmit(); }}
                    placeholder="abc1234..."
                    className="flex-1 rounded border border-border bg-navy-950 px-2 py-1 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
                    autoFocus
                  />
                  <button
                    onClick={handleCommitHashSubmit}
                    disabled={!commitHashInput.trim()}
                    className="rounded bg-accent px-2 py-1 text-[10px] font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
                  >
                    Go
                  </button>
                </div>
              </div>

              {/* Branches */}
              {branches && branches.length > 0 && (
                <div className="border-b border-border p-2">
                  <label className="mb-1 flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-text-muted">
                    <GitBranch size={10} />
                    Branches
                  </label>
                  <div className="max-h-[120px] overflow-y-auto">
                    {branches.map((branch) => (
                      <button
                        key={branch.name}
                        onClick={() => handleSelectRef(branch.name)}
                        className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs transition-colors ${
                          selectedRef === branch.name
                            ? 'bg-accent/15 text-accent'
                            : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                        }`}
                      >
                        <GitBranch size={11} className="flex-shrink-0" />
                        <span className="truncate">{branch.name}</span>
                        {branch.is_current && (
                          <span className="ml-auto text-[9px] text-accent">current</span>
                        )}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* Tags */}
              {tags && tags.length > 0 && (
                <div className="p-2">
                  <label className="mb-1 flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-text-muted">
                    <Tag size={10} />
                    Tags
                  </label>
                  <div className="max-h-[120px] overflow-y-auto">
                    {tags.map((tag) => (
                      <button
                        key={tag.name}
                        onClick={() => handleSelectRef(tag.name)}
                        className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs transition-colors ${
                          selectedRef === tag.name
                            ? 'bg-accent/15 text-accent'
                            : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                        }`}
                      >
                        <Tag size={11} className="flex-shrink-0" />
                        <span className="truncate">{tag.name}</span>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-hidden">
        {showSkeleton ? (
          <BlameViewSkeleton />
        ) : (
          <BlameView repoId={repoId!} blame={blame} />
        )}
      </div>
    </div>
  );
}

export default BlamePage;
