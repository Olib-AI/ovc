import { useState } from 'react';
import { Lock, X } from 'lucide-react';

interface UnlockModalProps {
  repoName: string;
  onUnlock: (password: string) => void;
  onClose: () => void;
  isUnlocking: boolean;
  error: string | null;
}

function UnlockModal({ repoName, onUnlock, onClose, isUnlocking, error }: UnlockModalProps) {
  const [password, setPassword] = useState('');

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (password) {
      onUnlock(password);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-label="Unlock Repository" className="w-full max-w-sm rounded-lg border border-border bg-navy-900 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Lock size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Unlock Repository</h2>
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
          <p className="mb-3 text-xs text-text-secondary">
            Enter the encryption password for <span className="font-semibold text-text-primary">{repoName}</span>
          </p>

          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Password"
            aria-label="Encryption password"
            className="w-full rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />

          {error && (
            <p className="mt-2 text-xs text-status-deleted">{error}</p>
          )}

          <button
            type="submit"
            disabled={!password || isUnlocking}
            className="mt-4 w-full rounded bg-accent py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isUnlocking ? 'Unlocking...' : 'Unlock'}
          </button>
        </form>
      </div>
    </div>
  );
}

export default UnlockModal;
