import { useState } from 'react';
import { KeyRound, Plus, Trash2, AlertTriangle, Eye, EyeOff, RefreshCw } from 'lucide-react';
import { useActionSecrets, usePutActionSecret, useDeleteActionSecret } from '../hooks/useActions.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';

interface ActionSecretsPanelProps {
  repoId: string;
}

function ActionSecretsPanel({ repoId }: ActionSecretsPanelProps) {
  const { data: secretsData, isLoading } = useActionSecrets(repoId);
  const putSecret = usePutActionSecret(repoId);
  const deleteSecret = useDeleteActionSecret(repoId);
  const toast = useToast();

  const [newName, setNewName] = useState('');
  const [newValue, setNewValue] = useState('');
  const [showValue, setShowValue] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  function handleAdd() {
    const trimmedName = newName.trim();
    const trimmedValue = newValue.trim();
    if (!trimmedName || !trimmedValue) return;
    putSecret.mutate(
      { name: trimmedName, value: trimmedValue },
      {
        onSuccess: () => {
          toast.success(`Secret "${trimmedName}" saved`);
          setNewName('');
          setNewValue('');
          setShowValue(false);
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : 'Failed to save secret');
        },
      },
    );
  }

  function handleDeleteConfirm(name: string) {
    deleteSecret.mutate(name, {
      onSuccess: () => {
        toast.success(`Secret "${name}" deleted`);
        setConfirmDelete(null);
      },
      onError: (err) => {
        toast.error(err instanceof Error ? err.message : 'Failed to delete secret');
        setConfirmDelete(null);
      },
    });
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-xl space-y-6 p-6">
        {/* Warning banner */}
        <div className="flex items-start gap-3 rounded-lg border border-yellow-500/30 bg-yellow-500/5 p-4">
          <AlertTriangle size={16} className="mt-0.5 flex-shrink-0 text-yellow-400" />
          <div className="text-xs text-yellow-300">
            <p className="font-semibold">Secret values cannot be viewed after creation.</p>
            <p className="mt-1 text-yellow-300/70">
              Secrets are encrypted at rest and only exposed to actions at runtime as environment
              variables. To update a secret, simply overwrite it with a new value.
            </p>
          </div>
        </div>

        {/* Secrets list */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center gap-2">
            <KeyRound size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Repository Secrets</h2>
          </div>

          {isLoading ? (
            <LoadingSpinner size={14} className="py-4" />
          ) : secretsData && secretsData.names.length > 0 ? (
            <div className="space-y-2">
              {secretsData.names.map((name) => (
                <div
                  key={name}
                  className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2.5"
                >
                  <div className="flex items-center gap-2.5 min-w-0">
                    <KeyRound size={12} className="flex-shrink-0 text-text-muted" />
                    <span className="font-mono text-xs font-medium text-text-primary truncate">{name}</span>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    <span
                      className="font-mono text-xs text-text-muted tracking-widest"
                      aria-label="Secret value hidden"
                    >
                      &#x25CF;&#x25CF;&#x25CF;&#x25CF;&#x25CF;&#x25CF;&#x25CF;&#x25CF;
                    </span>
                    <button
                      onClick={() => setConfirmDelete(name)}
                      disabled={deleteSecret.isPending}
                      className="rounded p-1.5 text-text-muted transition-colors hover:bg-red-400/10 hover:text-red-400 disabled:opacity-50"
                      title={`Delete secret ${name}`}
                      aria-label={`Delete secret ${name}`}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <p className="py-2 text-xs text-text-muted">
              No secrets configured. Add your first secret below.
            </p>
          )}
        </div>

        {/* Add secret form */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-4 flex items-center gap-2">
            <Plus size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Add Secret</h2>
          </div>

          <div className="space-y-3">
            <div>
              <label className="mb-1.5 block text-[11px] font-medium uppercase tracking-wider text-text-muted">
                Name
              </label>
              <input
                type="text"
                placeholder="SECRET_NAME"
                value={newName}
                onChange={(e) => setNewName(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, ''))}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && newName.trim() && newValue.trim()) handleAdd();
                }}
                className="w-full rounded border border-border bg-navy-950 px-3 py-2 font-mono text-xs text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none"
              />
              <p className="mt-1 text-[10px] text-text-muted">
                Uppercase letters, digits, and underscores only.
              </p>
            </div>

            <div>
              <label className="mb-1.5 block text-[11px] font-medium uppercase tracking-wider text-text-muted">
                Value
              </label>
              <div className="relative">
                <input
                  type={showValue ? 'text' : 'password'}
                  placeholder="Enter secret value"
                  value={newValue}
                  onChange={(e) => setNewValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && newName.trim() && newValue.trim()) handleAdd();
                  }}
                  className="w-full rounded border border-border bg-navy-950 px-3 py-2 pr-9 text-xs text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none"
                />
                <button
                  type="button"
                  onClick={() => setShowValue((v) => !v)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-text-muted transition-colors hover:text-text-primary"
                  aria-label={showValue ? 'Hide value' : 'Show value'}
                >
                  {showValue ? <EyeOff size={13} /> : <Eye size={13} />}
                </button>
              </div>
            </div>

            <button
              onClick={handleAdd}
              disabled={putSecret.isPending || !newName.trim() || !newValue.trim()}
              className="flex w-full items-center justify-center gap-2 rounded bg-accent px-4 py-2 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
            >
              {putSecret.isPending ? (
                <RefreshCw size={13} className="animate-spin" />
              ) : (
                <KeyRound size={13} />
              )}
              {putSecret.isPending ? 'Saving...' : 'Save Secret'}
            </button>
          </div>
        </div>
      </div>

      {/* Confirm delete modal */}
      {confirmDelete && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div
            className="fixed inset-0 bg-navy-950/70"
            onClick={() => setConfirmDelete(null)}
          />
          <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-5 shadow-2xl">
            <div className="mb-4 flex items-start gap-3">
              <AlertTriangle size={18} className="mt-0.5 flex-shrink-0 text-red-400" />
              <div>
                <p className="text-sm font-semibold text-text-primary">Delete secret?</p>
                <p className="mt-1 text-xs text-text-secondary">
                  Permanently delete{' '}
                  <span className="font-mono text-accent">{confirmDelete}</span>? This cannot be
                  undone. Any actions that depend on this secret will fail.
                </p>
              </div>
            </div>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDelete(null)}
                className="rounded px-3 py-1.5 text-xs text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={() => handleDeleteConfirm(confirmDelete)}
                disabled={deleteSecret.isPending}
                className="flex items-center gap-1.5 rounded bg-red-500/20 px-3 py-1.5 text-xs font-medium text-red-400 transition-colors hover:bg-red-500/30 disabled:opacity-50"
              >
                {deleteSecret.isPending && <RefreshCw size={11} className="animate-spin" />}
                Delete Secret
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default ActionSecretsPanel;
