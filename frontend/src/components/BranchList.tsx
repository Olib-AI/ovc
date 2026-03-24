import { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import { GitBranch, Plus, Trash2, GitMerge, X, Search, RotateCcw, ChevronDown, Loader2, GitPullRequest, Copy, Check } from 'lucide-react';
import type { BranchInfo } from '../api/types.ts';

interface BranchListProps {
  branches: BranchInfo[];
  onCreateBranch: (name: string, startPoint?: string) => void;
  onDeleteBranch: (name: string) => void;
  onCheckout: (name: string) => void;
  onMerge: (target: string, source: string) => void;
  onRebase?: (onto: string) => void;
  onCompare?: (name: string) => void;
  isCreating: boolean;
  isDeleting: boolean;
  externalShowCreate?: boolean;
  onExternalShowCreateConsumed?: () => void;
}

type ConfirmAction =
  | { kind: 'merge'; source: string; target: string }
  | { kind: 'rebase'; onto: string }
  | { kind: 'delete'; name: string };

function BranchList({
  branches,
  onCreateBranch,
  onDeleteBranch,
  onCheckout,
  onMerge,
  onRebase,
  onCompare,
  isCreating,
  isDeleting,
  externalShowCreate,
  onExternalShowCreateConsumed,
}: BranchListProps) {
  const [newBranchName, setNewBranchName] = useState('');
  const [startPoint, setStartPoint] = useState('');
  const [startPointInput, setStartPointInput] = useState('');
  const [showStartPointDropdown, setShowStartPointDropdown] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [autoCheckout, setAutoCheckout] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null);
  const [copiedBranch, setCopiedBranch] = useState<string | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const checkoutTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    return () => {
      clearTimeout(copyTimerRef.current);
      clearTimeout(checkoutTimerRef.current);
    };
  }, []);

  const handleCopyBranch = useCallback((name: string) => {
    void navigator.clipboard.writeText(name);
    setCopiedBranch(name);
    clearTimeout(copyTimerRef.current);
    copyTimerRef.current = setTimeout(() => setCopiedBranch(null), 1500);
  }, []);

  useEffect(() => {
    if (externalShowCreate) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setShowCreate(true);
      onExternalShowCreateConsumed?.();
    }
  }, [externalShowCreate, onExternalShowCreateConsumed]);

  const currentBranch = branches.find((b) => b.is_current);
  const showSearch = branches.length > 5;

  const filteredBranches = useMemo(() => {
    if (!searchQuery.trim()) return branches;
    const q = searchQuery.toLowerCase();
    return branches.filter((b) => b.name.toLowerCase().includes(q));
  }, [branches, searchQuery]);

  function handleCreate() {
    if (newBranchName.trim()) {
      const effectiveStartPoint = startPointInput.trim() || startPoint;
      const sp = effectiveStartPoint && effectiveStartPoint !== (currentBranch?.name ?? '') ? effectiveStartPoint : undefined;
      const branchName = newBranchName.trim();
      const shouldCheckout = autoCheckout;
      onCreateBranch(branchName, sp);
      if (shouldCheckout) {
        // Checkout after a short delay to allow branch creation to propagate
        clearTimeout(checkoutTimerRef.current);
        checkoutTimerRef.current = setTimeout(() => onCheckout(branchName), 300);
      }
      setNewBranchName('');
      setStartPoint('');
      setStartPointInput('');
      setShowCreate(false);
    }
  }

  function handleConfirm() {
    if (!confirmAction) return;
    switch (confirmAction.kind) {
      case 'merge':
        onMerge(confirmAction.target, confirmAction.source);
        break;
      case 'rebase':
        onRebase?.(confirmAction.onto);
        break;
      case 'delete':
        onDeleteBranch(confirmAction.name);
        break;
    }
    setConfirmAction(null);
  }

  function getConfirmMessage(): string {
    if (!confirmAction) return '';
    switch (confirmAction.kind) {
      case 'merge':
        return `Merge "${confirmAction.source}" into "${confirmAction.target}"?`;
      case 'rebase':
        return `Rebase "${currentBranch?.name ?? ''}" onto "${confirmAction.onto}"?`;
      case 'delete':
        return `Delete branch "${confirmAction.name}"?`;
    }
  }

  return (
    <div>
      <div className="flex items-center justify-between px-3 py-2">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-text-muted">
          <GitBranch size={13} />
          Branches
        </h3>
        <button
          onClick={() => {
            const next = !showCreate;
            setShowCreate(next);
            if (!next) setNewBranchName('');
          }}
          className="rounded p-0.5 text-text-muted transition-colors hover:text-accent"
        >
          {showCreate ? <X size={13} /> : <Plus size={13} />}
        </button>
      </div>

      {showCreate && (
        <div className="space-y-1.5 px-3 pb-2">
          <input
            value={newBranchName}
            onChange={(e) => setNewBranchName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
            placeholder="Branch name"
            aria-label="Branch name"
            className="w-full rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          {/* Start point: text input for branch/tag/SHA */}
          <input
            value={startPointInput}
            onChange={(e) => {
              setStartPointInput(e.target.value);
              if (e.target.value.trim()) setStartPoint('');
            }}
            placeholder="branch, tag, or commit SHA"
            aria-label="Start point (branch, tag, or commit SHA)"
            className="w-full rounded border border-border bg-navy-950 px-2 py-1 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
          />
          {/* From branch selector */}
          <div className="relative">
            <button
              type="button"
              onClick={() => setShowStartPointDropdown(!showStartPointDropdown)}
              className="flex w-full items-center justify-between rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-secondary transition-colors hover:border-accent/40"
            >
              <span className="truncate">
                From: <span className="font-mono text-text-primary">{startPointInput.trim() || startPoint || currentBranch?.name || 'HEAD'}</span>
              </span>
              <ChevronDown size={11} className="flex-shrink-0 text-text-muted" />
            </button>
            {showStartPointDropdown && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={() => setShowStartPointDropdown(false)}
                />
                <div className="absolute left-0 right-0 top-full z-20 mt-1 max-h-[200px] overflow-y-auto rounded-md border border-border bg-navy-800 py-1 shadow-lg">
                  {branches.map((b) => (
                    <button
                      key={b.name}
                      onClick={() => {
                        setStartPoint(b.name);
                        setStartPointInput('');
                        setShowStartPointDropdown(false);
                      }}
                      className={`flex w-full items-center px-3 py-1.5 text-left text-xs transition-colors ${
                        (startPoint || currentBranch?.name) === b.name
                          ? 'bg-accent/10 text-accent'
                          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                      }`}
                    >
                      <span className="truncate font-mono">{b.name}</span>
                      {b.is_current && (
                        <span className="ml-auto flex-shrink-0 text-[10px] text-accent/60">HEAD</span>
                      )}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
          <label className="flex cursor-pointer items-center gap-1.5 text-xs text-text-secondary">
            <input
              type="checkbox"
              checked={autoCheckout}
              onChange={(e) => setAutoCheckout(e.target.checked)}
              className="rounded border-border accent-accent"
            />
            Switch to new branch
          </label>
          <button
            onClick={handleCreate}
            disabled={isCreating || !newBranchName.trim()}
            className="flex w-full items-center justify-center gap-1.5 rounded bg-accent px-2 py-1 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isCreating && <Loader2 size={11} className="animate-spin" />}
            {isCreating ? 'Creating...' : 'Create'}
          </button>
        </div>
      )}

      {showSearch && (
        <div className="px-3 pb-2">
          <div className="flex items-center gap-1.5 rounded border border-border bg-navy-950 px-2 py-1">
            <Search size={11} className="flex-shrink-0 text-text-muted" />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Filter branches..."
              aria-label="Filter branches"
              className="flex-1 bg-transparent text-xs text-text-primary placeholder-text-muted focus:outline-none"
            />
          </div>
        </div>
      )}

      {branches.length === 0 && (
        <div className="flex flex-col items-center gap-1.5 px-3 py-4 text-center">
          <GitBranch size={20} className="text-text-muted/40" />
          <p className="text-xs text-text-muted">No branches</p>
        </div>
      )}

      <div className="space-y-px px-2">
        {filteredBranches.map((branch) => (
          <div
            key={branch.name}
            className={`group flex items-center gap-1.5 rounded px-2 py-1 ${
              branch.is_current ? 'bg-accent/10' : 'hover:bg-surface-hover'
            }`}
          >
            <GitBranch
              size={13}
              className={branch.is_current ? 'text-accent' : 'text-text-muted'}
            />
            <button
              onClick={() => !branch.is_current && onCheckout(branch.name)}
              className={`flex min-w-0 flex-1 items-center gap-1 text-left text-xs ${
                branch.is_current ? 'font-semibold text-accent' : 'text-text-secondary'
              }`}
            >
              <span className="truncate">{branch.name}</span>
              {branch.is_current && (
                <span className="flex-shrink-0 text-[10px] text-accent/60">*</span>
              )}
            </button>
            <div className="flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
              <button
                onClick={() => handleCopyBranch(branch.name)}
                className="rounded p-0.5 text-text-muted hover:text-accent"
                title={`Copy "${branch.name}"`}
                aria-label={`Copy branch name ${branch.name}`}
              >
                {copiedBranch === branch.name ? <Check size={12} className="text-status-added" /> : <Copy size={12} />}
              </button>
              {!branch.is_current && onCompare && (
                <button
                  onClick={() => onCompare(branch.name)}
                  className="rounded p-0.5 text-text-muted hover:text-accent"
                  title={`Compare ${branch.name} against current branch`}
                  aria-label={`Open pull request view for ${branch.name}`}
                >
                  <GitPullRequest size={12} />
                </button>
              )}
              {!branch.is_current && currentBranch && onRebase && (
                <button
                  onClick={() =>
                    setConfirmAction({ kind: 'rebase', onto: branch.name })
                  }
                  className="rounded p-0.5 text-text-muted hover:text-accent"
                  title={`Rebase ${currentBranch.name} onto ${branch.name}`}
                  aria-label={`Rebase ${currentBranch.name} onto ${branch.name}`}
                >
                  <RotateCcw size={12} />
                </button>
              )}
              {!branch.is_current && currentBranch && (
                <button
                  onClick={() =>
                    setConfirmAction({
                      kind: 'merge',
                      source: branch.name,
                      target: currentBranch.name,
                    })
                  }
                  className="rounded p-0.5 text-text-muted hover:text-accent"
                  title={`Merge ${branch.name} into ${currentBranch.name}`}
                  aria-label={`Merge ${branch.name} into ${currentBranch.name}`}
                >
                  <GitMerge size={12} />
                </button>
              )}
              {!branch.is_current && (
                <button
                  onClick={() =>
                    setConfirmAction({ kind: 'delete', name: branch.name })
                  }
                  disabled={isDeleting}
                  className="rounded p-0.5 text-text-muted hover:text-status-deleted"
                  title="Delete branch"
                  aria-label={`Delete branch ${branch.name}`}
                >
                  <Trash2 size={12} />
                </button>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* Confirmation modal */}
      {confirmAction && (
        <div className="mx-3 mt-2 rounded border border-accent/30 bg-accent/5 p-2">
          <p className="text-xs text-text-secondary">{getConfirmMessage()}</p>
          <div className="mt-2 flex gap-1">
            <button
              onClick={handleConfirm}
              className={`rounded px-2 py-1 text-xs font-medium ${
                confirmAction.kind === 'delete'
                  ? 'bg-status-deleted/20 text-status-deleted hover:bg-status-deleted/30'
                  : 'bg-accent text-navy-950 hover:bg-accent-light'
              }`}
            >
              {confirmAction.kind === 'delete' ? 'Delete' : confirmAction.kind === 'merge' ? 'Merge' : 'Rebase'}
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
    </div>
  );
}

export default BranchList;
