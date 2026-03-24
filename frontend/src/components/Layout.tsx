import { useState, useCallback, useEffect, useRef } from 'react';
import { Outlet, useNavigate, useLocation, useParams } from 'react-router-dom';
import { X } from 'lucide-react';
import Sidebar from './Sidebar.tsx';
import CreateRepoModal from './CreateRepoModal.tsx';
import CommandPalette from './CommandPalette.tsx';
import ErrorBoundary from './ErrorBoundary.tsx';
import RepoContextBar from './RepoContextBar.tsx';
import { useCreateRepo } from '../hooks/useRepo.ts';
import { useCommandPalette } from '../contexts/CommandPaletteContext.tsx';
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut.ts';
import { useCacheGC } from '../hooks/useCacheGC.ts';
import type { PaletteCommand } from '../contexts/CommandPaletteContext.tsx';
import { useQueryClient } from '@tanstack/react-query';
import axios from 'axios';

interface ShortcutEntry {
  keys: string;
  description: string;
}

interface ShortcutGroup {
  label: string;
  shortcuts: ShortcutEntry[];
}

const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    label: 'General',
    shortcuts: [
      { keys: '\u2318 K', description: 'Open command palette' },
      { keys: '?', description: 'Show keyboard shortcuts' },
      { keys: 'Escape', description: 'Close modal / dismiss dialog' },
    ],
  },
  {
    label: 'Navigation',
    shortcuts: [
      { keys: 'G H', description: 'Go to History' },
      { keys: 'G A', description: 'Go to Actions' },
      { keys: 'G S', description: 'Go to Search' },
      { keys: 'G P', description: 'Go to Pull Requests' },
      { keys: 'G D', description: 'Go to Dependencies' },
    ],
  },
  {
    label: 'Git Operations',
    shortcuts: [
      { keys: '\u2318 \u21E7 P', description: 'Push to remote' },
      { keys: '\u2318 \u21E7 L', description: 'Pull from remote' },
    ],
  },
  {
    label: 'Commit Graph',
    shortcuts: [
      { keys: 'Right-click', description: 'Open commit context menu' },
      { keys: 'Click commit', description: 'View commit details' },
    ],
  },
  {
    label: 'Search',
    shortcuts: [
      { keys: '/', description: 'Focus search input (on search pages)' },
    ],
  },
];

function KeyboardShortcutsModal({ onClose }: { onClose: () => void }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="fixed inset-0 bg-navy-950/70" onClick={onClose} />
      <div className="relative z-10 w-full max-w-lg overflow-hidden rounded-xl border border-border bg-navy-800 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border px-5 py-3">
          <h2 className="text-sm font-semibold text-text-primary">Keyboard Shortcuts</h2>
          <button
            onClick={onClose}
            className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>
        <div className="max-h-[70vh] overflow-y-auto px-5 py-4">
          {/* Command palette highlight */}
          <div className="mb-4 flex items-center justify-between rounded-lg border border-accent/30 bg-accent/5 px-4 py-3">
            <div>
              <p className="text-xs font-semibold text-text-primary">Command Palette</p>
              <p className="mt-0.5 text-[11px] text-text-muted">
                Search and run any action in the app
              </p>
            </div>
            <kbd className="flex items-center gap-1 rounded border border-accent/30 bg-navy-900 px-2 py-1 font-mono text-xs font-semibold text-accent">
              <span className="text-[11px]">&#x2318;</span>K
            </kbd>
          </div>

          <div className="space-y-5">
            {SHORTCUT_GROUPS.map((group) => (
              <div key={group.label}>
                <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
                  {group.label}
                </p>
                <table className="w-full">
                  <tbody>
                    {group.shortcuts.map(({ keys, description }) => (
                      <tr key={keys} className="border-b border-border/20 last:border-0">
                        <td className="py-2 pr-4 align-middle">
                          <kbd className="inline-flex items-center gap-0.5 rounded border border-border bg-navy-900 px-2 py-0.5 font-mono text-[11px] text-text-secondary">
                            {keys}
                          </kbd>
                        </td>
                        <td className="py-2 align-middle text-xs text-text-secondary">{description}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))}
          </div>

          <p className="mt-4 text-[10px] text-text-muted">
            Press <kbd className="rounded border border-border bg-navy-900 px-1 py-0.5 font-mono text-[10px]">?</kbd> at any time to show this dialog. Press <kbd className="rounded border border-border bg-navy-900 px-1 py-0.5 font-mono text-[10px]">Esc</kbd> to close.
          </p>
        </div>
      </div>
    </div>
  );
}

function Layout() {
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const createRepo = useCreateRepo();
  const navigate = useNavigate();
  const location = useLocation();
  const { repoId } = useParams<{ repoId: string }>();
  const [createError, setCreateError] = useState<string | null>(null);
  const { registerCommands, unregisterCommands } = useCommandPalette();
  const queryClient = useQueryClient();

  // Periodic cache garbage collection — keeps memory bounded during 12+ hr sessions
  useCacheGC();

  // Clear previous repo's inactive cache when navigating between repos (Fix 5)
  const prevRepoRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (prevRepoRef.current && prevRepoRef.current !== repoId) {
      queryClient.removeQueries({
        queryKey: ['repo', prevRepoRef.current],
        type: 'inactive',
      });
    }
    prevRepoRef.current = repoId;
  }, [repoId, queryClient]);

  const toggleShortcuts = useCallback(() => setShowShortcuts((v) => !v), []);
  useKeyboardShortcut('?', toggleShortcuts);

  // Register the "Keyboard Shortcuts" command in the command palette
  useEffect(() => {
    const commands: PaletteCommand[] = [
      {
        id: 'shortcuts-help',
        label: 'Keyboard Shortcuts',
        category: 'Help',
        shortcut: '?',
        action: () => setShowShortcuts(true),
      },
    ];
    registerCommands(commands);
    return () => unregisterCommands(commands.map((c) => c.id));
  }, [registerCommands, unregisterCommands]);

  // Show context bar on repo sub-pages, but not on the main repo page
  // which has its own full Header component
  const isRepoSubPage =
    !!repoId &&
    location.pathname.startsWith(`/repo/${repoId}/`);

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

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar onCreateRepo={() => setShowCreateModal(true)} />
      {/* On mobile the sidebar becomes a fixed overlay, so main must fill the full width */}
      <main className="flex flex-1 flex-col overflow-hidden min-w-0">
        {isRepoSubPage && <RepoContextBar />}
        <div className="flex-1 overflow-hidden">
          <ErrorBoundary key={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </div>
      </main>

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

      <CommandPalette />
      {showShortcuts && <KeyboardShortcutsModal onClose={() => setShowShortcuts(false)} />}
    </div>
  );
}

export default Layout;
