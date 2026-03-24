import { useState, useCallback, useRef, useEffect } from 'react';
import { Tag, Plus, Trash2, X, ChevronDown, ChevronRight, Copy, Check } from 'lucide-react';
import type { TagInfo } from '../api/types.ts';

interface TagListProps {
  tags: TagInfo[];
  onCreateTag: (name: string, message?: string, commitId?: string) => void;
  onDeleteTag: (name: string) => void;
  isCreating: boolean;
}

function TagList({ tags, onCreateTag, onDeleteTag, isCreating }: TagListProps) {
  const [showCreate, setShowCreate] = useState(false);
  const [newTagName, setNewTagName] = useState('');
  const [newTagMessage, setNewTagMessage] = useState('');
  const [newTagCommit, setNewTagCommit] = useState('');
  const [showMessageField, setShowMessageField] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [copiedTag, setCopiedTag] = useState<string | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    return () => clearTimeout(copyTimerRef.current);
  }, []);

  const handleCopyTag = useCallback((name: string) => {
    void navigator.clipboard.writeText(name);
    setCopiedTag(name);
    clearTimeout(copyTimerRef.current);
    copyTimerRef.current = setTimeout(() => setCopiedTag(null), 1500);
  }, []);

  function handleCreate() {
    if (newTagName.trim()) {
      const message = newTagMessage.trim() || undefined;
      const commitId = newTagCommit.trim() || undefined;
      onCreateTag(newTagName.trim(), message, commitId);
      setNewTagName('');
      setNewTagMessage('');
      setNewTagCommit('');
      setShowMessageField(false);
      setShowCreate(false);
    }
  }

  return (
    <div className="border-t border-border pt-2">
      <div className="flex items-center justify-between px-3 py-1">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-text-muted">
          <Tag size={13} />
          Tags
        </h3>
        <button
          onClick={() => setShowCreate(!showCreate)}
          className="rounded p-0.5 text-text-muted transition-colors hover:text-accent"
        >
          {showCreate ? <X size={13} /> : <Plus size={13} />}
        </button>
      </div>

      {showCreate && (
        <div className="space-y-1.5 px-3 pb-2">
          <input
            value={newTagName}
            onChange={(e) => setNewTagName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && !showMessageField && handleCreate()}
            placeholder="Tag name"
            aria-label="Tag name"
            className="w-full rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />
          <input
            value={newTagCommit}
            onChange={(e) => setNewTagCommit(e.target.value)}
            placeholder="Commit (leave empty for HEAD)"
            aria-label="Commit reference for tag"
            className="w-full rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none font-mono"
          />
          <button
            type="button"
            onClick={() => setShowMessageField(!showMessageField)}
            className="flex items-center gap-1 text-[11px] text-text-muted transition-colors hover:text-text-secondary"
          >
            {showMessageField ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
            Annotated tag message
          </button>
          {showMessageField && (
            <textarea
              value={newTagMessage}
              onChange={(e) => setNewTagMessage(e.target.value)}
              placeholder="Optional message for annotated tag..."
              aria-label="Annotated tag message"
              rows={2}
              className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          )}
          <button
            onClick={handleCreate}
            disabled={isCreating || !newTagName.trim()}
            className="w-full rounded bg-accent px-2 py-1 text-xs font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50"
          >
            Create
          </button>
        </div>
      )}

      {tags.length === 0 && (
        <div className="flex flex-col items-center gap-1.5 px-3 py-4 text-center">
          <Tag size={20} className="text-text-muted/40" />
          <p className="text-xs text-text-muted">No tags yet</p>
          <p className="text-[11px] text-text-muted/60">Create your first tag</p>
        </div>
      )}

      <div className="space-y-px overflow-hidden px-2">
        {tags.map((tag) => (
          <div
            key={tag.name}
            className="group flex min-w-0 items-center gap-1.5 rounded px-2 py-1 hover:bg-surface-hover"
          >
            <Tag size={12} className="flex-shrink-0 text-accent/60" />
            <div className="min-w-0 flex-1">
              <span className="block truncate text-xs text-text-secondary">{tag.name}</span>
              {tag.message && (
                <span className="block truncate text-[10px] text-text-muted">{tag.message}</span>
              )}
            </div>
            <span className="font-mono text-[10px] text-text-muted">
              {tag.commit_id.slice(0, 8)}
            </span>
            <button
              onClick={() => handleCopyTag(tag.name)}
              className="rounded p-0.5 text-text-muted opacity-0 transition-opacity hover:text-accent group-hover:opacity-100"
              title={`Copy "${tag.name}"`}
              aria-label={`Copy tag name ${tag.name}`}
            >
              {copiedTag === tag.name ? <Check size={11} className="text-status-added" /> : <Copy size={11} />}
            </button>
            <button
              onClick={() => setConfirmDelete(tag.name)}
              className="rounded p-0.5 text-text-muted opacity-0 transition-opacity hover:text-status-deleted group-hover:opacity-100"
              title="Delete tag"
              aria-label={`Delete tag ${tag.name}`}
            >
              <Trash2 size={11} />
            </button>
          </div>
        ))}
      </div>

      {/* Confirmation modal */}
      {confirmDelete && (
        <div className="mx-3 mt-2 rounded border border-status-deleted/30 bg-status-deleted/5 p-2">
          <p className="text-xs text-text-secondary">
            Delete tag &quot;{confirmDelete}&quot;?
          </p>
          <div className="mt-2 flex gap-1">
            <button
              onClick={() => {
                onDeleteTag(confirmDelete);
                setConfirmDelete(null);
              }}
              className="rounded bg-status-deleted/20 px-2 py-1 text-xs font-medium text-status-deleted hover:bg-status-deleted/30"
            >
              Delete
            </button>
            <button
              onClick={() => setConfirmDelete(null)}
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

export default TagList;
