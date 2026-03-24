import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useCommitDiff, useNote, useSetNote, useDeleteNote, useDescribeCommit } from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import type { CommitInfo, ResetMode } from '../api/types.ts';
import DiffViewer from './DiffViewer.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';
import ArchiveDialog from './ArchiveDialog.tsx';
import {
  GitCommitVertical,
  User,
  Clock,
  Hash,
  ShieldCheck,
  ShieldAlert,
  GitBranch,
  CherryIcon,
  Tag,
  Copy,
  FolderTree,
  StickyNote,
  Pencil,
  Trash2,
  Save,
  X,
  Bookmark,
  RotateCcw,
  AlertTriangle,
  Archive,
} from 'lucide-react';

interface CommitDetailProps {
  repoId: string;
  repoName?: string;
  commit: CommitInfo;
  onCherryPick?: (commitId: string) => void;
  onRevert?: (commitId: string) => void;
  onCreateBranch?: (name: string, commitId: string) => void;
  onCreateTag?: (name: string, commitId: string, message?: string) => void;
  onBrowseFiles?: (commitId: string) => void;
  onReset?: (commitId: string, mode: ResetMode) => void;
}

function SignatureBadge({ commit }: { commit: CommitInfo }) {
  if (commit.signature_status === 'verified') {
    return (
      <div className="mt-2 flex items-start gap-2 rounded border border-green-500/20 bg-green-500/5 px-3 py-2">
        <ShieldCheck size={16} className="mt-0.5 flex-shrink-0 text-green-400" />
        <div className="text-xs">
          <span className="font-semibold text-green-400">Verified</span>
          {commit.signer_identity && (
            <p className="mt-0.5 text-text-muted">Signed by: {commit.signer_identity}</p>
          )}
          {commit.signer_fingerprint && (
            <p className="mt-0.5 font-mono text-text-muted">Key: {commit.signer_fingerprint}</p>
          )}
        </div>
      </div>
    );
  }
  if (commit.signature_status === 'unverified') {
    return (
      <div className="mt-2 flex items-start gap-2 rounded border border-red-500/20 bg-red-500/5 px-3 py-2">
        <ShieldAlert size={16} className="mt-0.5 flex-shrink-0 text-red-400" />
        <div className="text-xs">
          <span className="font-semibold text-red-400">Unverified</span>
          <p className="mt-0.5 text-text-muted">
            This commit has a signature that could not be verified against any authorized key.
          </p>
        </div>
      </div>
    );
  }
  return null;
}

type InlineForm = 'branch' | 'tag' | null;

interface ActionBarProps {
  repoId: string;
  repoName?: string;
  commit: CommitInfo;
  onCherryPick?: (commitId: string) => void;
  onRevert?: (commitId: string) => void;
  onCreateBranch?: (name: string, commitId: string) => void;
  onCreateTag?: (name: string, commitId: string, message?: string) => void;
  onBrowseFiles?: (commitId: string) => void;
  onReset?: (commitId: string, mode: ResetMode) => void;
}

function CommitActionBar({
  repoId,
  repoName,
  commit,
  onCherryPick,
  onRevert,
  onCreateBranch,
  onCreateTag,
  onBrowseFiles,
  onReset,
}: ActionBarProps) {
  const [activeForm, setActiveForm] = useState<InlineForm>(null);
  const [inputValue, setInputValue] = useState('');
  const [tagMessage, setTagMessage] = useState('');
  const [showTagMessage, setShowTagMessage] = useState(false);
  const [copied, setCopied] = useState(false);
  const [confirmRevert, setConfirmRevert] = useState(false);
  const [resetConfirm, setResetConfirm] = useState<ResetMode | null>(null);
  const [showArchive, setShowArchive] = useState(false);

  function handleCopyHash() {
    void navigator.clipboard.writeText(commit.id);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  function handleSubmitForm() {
    const value = inputValue.trim();
    if (!value) return;
    if (activeForm === 'branch') {
      onCreateBranch?.(value, commit.id);
    } else if (activeForm === 'tag') {
      const msg = tagMessage.trim() || undefined;
      onCreateTag?.(value, commit.id, msg);
    }
    setInputValue('');
    setTagMessage('');
    setShowTagMessage(false);
    setActiveForm(null);
  }

  function toggleForm(form: InlineForm) {
    if (activeForm === form) {
      setActiveForm(null);
      setInputValue('');
      setTagMessage('');
      setShowTagMessage(false);
    } else {
      setActiveForm(form);
      setInputValue('');
      setTagMessage('');
      setShowTagMessage(false);
    }
  }

  const buttonClass =
    'flex items-center gap-1 rounded px-2 py-1 text-[11px] text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary';

  return (
    <div className="border-b border-border bg-navy-800/30 px-5 py-2">
      <div className="flex flex-wrap items-center gap-1">
        {onCreateBranch && (
          <button
            onClick={() => toggleForm('branch')}
            className={`${buttonClass} ${activeForm === 'branch' ? 'bg-accent/10 text-accent' : ''}`}
            title="Create branch from this commit"
          >
            <GitBranch size={12} />
            Branch
          </button>
        )}
        {onCherryPick && (
          <button
            onClick={() => onCherryPick(commit.id)}
            className={buttonClass}
            title="Cherry-pick this commit"
          >
            <CherryIcon size={12} />
            Cherry-pick
          </button>
        )}
        {onRevert && (
          <button
            onClick={() => setConfirmRevert(true)}
            className={buttonClass}
            title="Revert this commit"
          >
            <RotateCcw size={12} />
            Revert
          </button>
        )}
        {onCreateTag && (
          <button
            onClick={() => toggleForm('tag')}
            className={`${buttonClass} ${activeForm === 'tag' ? 'bg-accent/10 text-accent' : ''}`}
            title="Create tag at this commit"
          >
            <Tag size={12} />
            Tag
          </button>
        )}
        <button
          onClick={handleCopyHash}
          className={buttonClass}
          title="Copy full commit hash"
        >
          <Copy size={12} />
          {copied ? 'Copied' : 'Copy Hash'}
        </button>
        {onBrowseFiles && (
          <button
            onClick={() => onBrowseFiles(commit.id)}
            className={buttonClass}
            title="Browse files at this commit"
          >
            <FolderTree size={12} />
            Browse Files
          </button>
        )}
        <button
          onClick={() => setShowArchive(true)}
          className={buttonClass}
          title="Export archive of this commit"
        >
          <Archive size={12} />
          Export
        </button>
        {onReset && (
          <button
            onClick={() => setResetConfirm(resetConfirm ? null : 'mixed')}
            className={`${buttonClass} ${resetConfirm !== null ? 'bg-status-deleted/10 text-status-deleted' : ''}`}
            title="Reset HEAD to this commit"
          >
            <RotateCcw size={12} />
            Reset
          </button>
        )}
      </div>

      {activeForm && (
        <div className="mt-2 space-y-2">
          <div className="flex items-center gap-2">
            <input
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !showTagMessage && handleSubmitForm()}
              placeholder={activeForm === 'branch' ? 'Branch name...' : 'Tag name...'}
              className="w-48 rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
              autoFocus
            />
            <button
              onClick={handleSubmitForm}
              disabled={!inputValue.trim()}
              className="rounded bg-accent px-2 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
            >
              Create
            </button>
            <button
              onClick={() => { setActiveForm(null); setInputValue(''); setTagMessage(''); setShowTagMessage(false); }}
              className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary"
            >
              Cancel
            </button>
          </div>
          {activeForm === 'tag' && !showTagMessage && (
            <button
              onClick={() => setShowTagMessage(true)}
              className="text-[11px] text-accent/70 transition-colors hover:text-accent"
            >
              + Add annotation
            </button>
          )}
          {activeForm === 'tag' && showTagMessage && (
            <textarea
              value={tagMessage}
              onChange={(e) => setTagMessage(e.target.value)}
              placeholder="Tag message (optional)..."
              rows={2}
              className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          )}
        </div>
      )}

      {confirmRevert && onRevert && (
        <div className="mt-2 flex items-center gap-2 rounded border border-status-deleted/30 bg-status-deleted/5 px-3 py-2">
          <RotateCcw size={12} className="flex-shrink-0 text-status-deleted" />
          <span className="text-xs text-text-secondary">
            Revert commit <span className="font-mono text-accent/70">{commit.short_id}</span>? This will create a new commit.
          </span>
          <button
            onClick={() => {
              onRevert(commit.id);
              setConfirmRevert(false);
            }}
            className="rounded bg-status-deleted px-2 py-1 text-xs font-medium text-white hover:opacity-90"
          >
            Revert
          </button>
          <button
            onClick={() => setConfirmRevert(false)}
            className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary"
          >
            Cancel
          </button>
        </div>
      )}

      {resetConfirm !== null && onReset && (
        <div className="mt-2 rounded border border-status-deleted/30 bg-status-deleted/5 px-3 py-2">
          <div className="mb-2 flex items-center gap-2">
            <AlertTriangle size={12} className="flex-shrink-0 text-status-deleted" />
            <span className="text-xs text-text-secondary">
              Reset HEAD to <span className="font-mono text-accent/70">{commit.short_id}</span>
            </span>
          </div>
          <div className="mb-2 flex gap-1">
            {(['soft', 'mixed', 'hard'] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setResetConfirm(mode)}
                className={`rounded px-2 py-1 text-[11px] font-medium transition-colors ${
                  resetConfirm === mode
                    ? mode === 'hard'
                      ? 'bg-status-deleted/20 text-status-deleted ring-1 ring-status-deleted/40'
                      : 'bg-accent/15 text-accent ring-1 ring-accent/40'
                    : 'bg-surface text-text-muted hover:text-text-secondary'
                }`}
              >
                {mode}
              </button>
            ))}
          </div>
          <p className="mb-2 text-[11px] text-text-muted">
            {resetConfirm === 'soft' && 'Keeps all changes staged. Safe.'}
            {resetConfirm === 'mixed' && 'Unstages changes but keeps them in working directory.'}
            {resetConfirm === 'hard' && 'Discards ALL uncommitted changes. Destructive.'}
          </p>
          <div className="flex items-center gap-2">
            <button
              onClick={() => {
                onReset(commit.id, resetConfirm);
                setResetConfirm(null);
              }}
              className={`rounded px-2 py-1 text-xs font-medium ${
                resetConfirm === 'hard'
                  ? 'bg-status-deleted text-white hover:opacity-90'
                  : 'bg-accent text-navy-950 hover:bg-accent-light'
              }`}
            >
              Reset ({resetConfirm})
            </button>
            <button
              onClick={() => setResetConfirm(null)}
              className="rounded px-2 py-1 text-xs text-text-muted hover:text-text-primary"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {showArchive && (
        <ArchiveDialog
          repoId={repoId}
          repoName={repoName ?? repoId}
          defaultRef={commit.id}
          onClose={() => setShowArchive(false)}
        />
      )}
    </div>
  );
}

interface NotesSectionProps {
  repoId: string;
  commitId: string;
}

function NotesSection({ repoId, commitId }: NotesSectionProps) {
  const { data: note, isLoading, isError } = useNote(repoId, commitId);
  const setNote = useSetNote(repoId);
  const deleteNoteMutation = useDeleteNote(repoId);
  const toast = useToast();

  const [editing, setEditing] = useState(false);
  const [noteText, setNoteText] = useState('');

  const hasNote = !isError && !!note?.message;

  function handleStartEdit() {
    setNoteText(hasNote ? note.message : '');
    setEditing(true);
  }

  function handleSave() {
    const trimmed = noteText.trim();
    if (!trimmed) return;
    setNote.mutate(
      { commitId, message: trimmed },
      {
        onSuccess: () => {
          toast.success(hasNote ? 'Note updated' : 'Note added');
          setEditing(false);
        },
        onError: (err: Error) => toast.error(err.message),
      },
    );
  }

  function handleDelete() {
    deleteNoteMutation.mutate(commitId, {
      onSuccess: () => {
        toast.success('Note deleted');
        setEditing(false);
        setNoteText('');
      },
      onError: (err: Error) => toast.error(err.message),
    });
  }

  if (isLoading) return null;

  return (
    <div className="border-b border-border bg-navy-800/30 px-5 py-3">
      <div className="flex items-center gap-2 mb-2">
        <StickyNote size={14} className="text-yellow-400" />
        <span className="text-xs font-semibold text-text-primary">Note</span>
      </div>

      {editing ? (
        <div className="space-y-2">
          <textarea
            value={noteText}
            onChange={(e) => setNoteText(e.target.value)}
            placeholder="Write a note for this commit..."
            rows={3}
            className="w-full resize-none rounded border border-border bg-navy-950 px-3 py-2 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          <div className="flex items-center gap-2">
            <button
              onClick={handleSave}
              disabled={!noteText.trim() || setNote.isPending}
              className="flex items-center gap-1 rounded bg-accent px-2 py-1 text-[11px] font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
            >
              <Save size={11} />
              {setNote.isPending ? 'Saving...' : 'Save'}
            </button>
            <button
              onClick={() => setEditing(false)}
              className="flex items-center gap-1 rounded px-2 py-1 text-[11px] text-text-muted hover:text-text-primary"
            >
              <X size={11} />
              Cancel
            </button>
          </div>
        </div>
      ) : hasNote ? (
        <div>
          <p className="whitespace-pre-wrap rounded border border-yellow-500/10 bg-yellow-500/5 px-3 py-2 text-xs text-text-secondary">
            {note.message}
          </p>
          <div className="mt-2 flex items-center gap-2">
            <button
              onClick={handleStartEdit}
              className="flex items-center gap-1 rounded px-2 py-1 text-[11px] text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            >
              <Pencil size={11} />
              Edit
            </button>
            <button
              onClick={handleDelete}
              disabled={deleteNoteMutation.isPending}
              className="flex items-center gap-1 rounded px-2 py-1 text-[11px] text-status-deleted transition-colors hover:bg-status-deleted/10 disabled:opacity-50"
            >
              <Trash2 size={11} />
              {deleteNoteMutation.isPending ? 'Deleting...' : 'Delete'}
            </button>
          </div>
        </div>
      ) : (
        <button
          onClick={handleStartEdit}
          className="flex items-center gap-1 rounded px-2 py-1 text-[11px] text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
        >
          <StickyNote size={11} />
          Add a note...
        </button>
      )}
    </div>
  );
}

function ParentLinks({ repoId, parentIds }: { repoId: string; parentIds: string[] }) {
  const navigate = useNavigate();
  return (
    <div className="mt-1 text-xs text-text-muted">
      Parents:{' '}
      {parentIds.map((pid, i) => (
        <span key={pid}>
          {i > 0 && ', '}
          <button
            onClick={() => navigate(`/repo/${repoId}/history?commit=${pid}`)}
            className="cursor-pointer font-mono text-accent/70 hover:text-accent"
          >
            {pid.slice(0, 12)}
          </button>
        </span>
      ))}
    </div>
  );
}

function CommitDetail({
  repoId,
  repoName,
  commit,
  onCherryPick,
  onRevert,
  onCreateBranch,
  onCreateTag,
  onBrowseFiles,
  onReset,
}: CommitDetailProps) {
  const { data: diff, isLoading } = useCommitDiff(repoId, commit.id);
  const { data: describe } = useDescribeCommit(repoId, commit.id);

  return (
    <div className="h-full overflow-y-auto">
      <div className="border-b border-border bg-navy-800/50 px-5 py-4">
        <div className="flex items-start gap-3">
          <GitCommitVertical size={20} className="mt-0.5 flex-shrink-0 text-accent" />
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-text-primary whitespace-pre-wrap">
              {commit.message}
            </p>
            <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-text-muted">
              <span className="flex items-center gap-1">
                <User size={12} />
                {commit.author.name} &lt;{commit.author.email}&gt;
              </span>
              <span className="flex items-center gap-1">
                <Clock size={12} />
                {new Date(commit.authored_at).toLocaleString()}
              </span>
              <span className="flex items-center gap-1">
                <Hash size={12} />
                <span className="font-mono">{commit.id}</span>
              </span>
              {describe?.description && (
                <span className="flex items-center gap-1" title="Nearest tag ancestor">
                  <Bookmark size={12} />
                  <span className="font-mono text-accent/80">{describe.description}</span>
                </span>
              )}
            </div>
            {commit.parent_ids.length > 0 && (
              <ParentLinks repoId={repoId} parentIds={commit.parent_ids} />
            )}
            <SignatureBadge commit={commit} />
          </div>
        </div>
      </div>

      <CommitActionBar
        repoId={repoId}
        repoName={repoName}
        commit={commit}
        onCherryPick={onCherryPick}
        onRevert={onRevert}
        onCreateBranch={onCreateBranch}
        onCreateTag={onCreateTag}
        onBrowseFiles={onBrowseFiles}
        onReset={onReset}
      />

      <NotesSection repoId={repoId} commitId={commit.id} />

      {isLoading && <LoadingSpinner className="py-8" message="Loading diff..." />}
      {diff && <DiffViewer diff={diff} repoId={repoId} commitId={commit.id} />}
    </div>
  );
}

export default CommitDetail;
