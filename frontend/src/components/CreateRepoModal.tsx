import { useState } from 'react';
import { Plus, X } from 'lucide-react';

interface CreateRepoModalProps {
  onClose: () => void;
  onCreate: (name: string, password: string) => void;
  isCreating: boolean;
  error: string | null;
}

function CreateRepoModal({ onClose, onCreate, isCreating, error }: CreateRepoModalProps) {
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (name.trim() && password) {
      onCreate(name.trim(), password);
    }
  }

  const nameValid = /^[a-zA-Z0-9._-]*$/.test(name);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-label="Create Repository" className="w-full max-w-sm rounded-lg border border-border bg-navy-900 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Plus size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Create Repository</h2>
          </div>
          <button
            onClick={onClose}
            aria-label="Close"
            className="rounded p-1 text-text-muted hover:text-text-primary"
          >
            <X size={16} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-4">
          <label className="mb-1 block text-xs text-text-muted">Repository Name</label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-project"
            className={`mb-3 w-full rounded border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:outline-none ${
              name && !nameValid
                ? 'border-status-deleted focus:border-status-deleted'
                : 'border-border focus:border-accent'
            }`}
            autoFocus
          />
          {name && !nameValid && (
            <p className="-mt-2 mb-2 text-[11px] text-status-deleted">
              Only alphanumeric, hyphens, underscores, and dots allowed
            </p>
          )}

          <label className="mb-1 block text-xs text-text-muted">Encryption Password</label>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Strong password"
            className="w-full rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
          />

          {error && (
            <p className="mt-2 text-xs text-status-deleted">{error}</p>
          )}

          <button
            type="submit"
            disabled={!name.trim() || !nameValid || !password || isCreating}
            className="mt-4 w-full rounded bg-accent py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isCreating ? 'Creating...' : 'Create Repository'}
          </button>
        </form>
      </div>
    </div>
  );
}

export default CreateRepoModal;
