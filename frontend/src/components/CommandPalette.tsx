import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { Search, Command, FileText, GitBranch, Clock } from 'lucide-react';
import { useCommandPalette } from '../contexts/CommandPaletteContext.tsx';
import type { PaletteCommand } from '../contexts/CommandPaletteContext.tsx';
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut.ts';

const RECENT_COMMANDS_KEY = 'ovc_recent_commands';
const MAX_RECENT = 5;

function loadRecentIds(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_COMMANDS_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as string[];
  } catch {
    return [];
  }
}

function saveRecentId(id: string) {
  try {
    const existing = loadRecentIds().filter((x) => x !== id);
    const next = [id, ...existing].slice(0, MAX_RECENT);
    localStorage.setItem(RECENT_COMMANDS_KEY, JSON.stringify(next));
  } catch {
    // localStorage may be unavailable
  }
}

function CommandPalette() {
  const { isOpen, open, close, commands } = useCommandPalette();
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [recentIds, setRecentIds] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useKeyboardShortcut('k', open, { meta: true });

  // Reload recent IDs each time the palette opens
  useEffect(() => {
    if (isOpen) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setRecentIds(loadRecentIds());
    }
  }, [isOpen]);

  /** Build the flat ordered list of commands to display */
  const flatItems = useMemo((): PaletteCommand[] => {
    const q = query.trim().toLowerCase();
    if (q) {
      // Filtered search: score by label match quality
      return commands
        .filter(
          (cmd) =>
            cmd.label.toLowerCase().includes(q) ||
            cmd.category.toLowerCase().includes(q),
        )
        .sort((a, b) => {
          const aLabel = a.label.toLowerCase();
          const bLabel = b.label.toLowerCase();
          // Exact prefix beats contains
          const aPrefix = aLabel.startsWith(q) ? 0 : 1;
          const bPrefix = bLabel.startsWith(q) ? 0 : 1;
          return aPrefix - bPrefix || aLabel.localeCompare(bLabel);
        });
    }

    // No query: show recent first, then all others grouped
    const recentSet = new Set(recentIds);
    const recentCmds = recentIds
      .map((id) => commands.find((c) => c.id === id))
      .filter((c): c is PaletteCommand => c !== undefined);
    const rest = commands.filter((c) => !recentSet.has(c.id));
    return [...recentCmds, ...rest];
  }, [commands, query, recentIds]);

  /** Group by category for display, injecting a "Recent" group at the top */
  const grouped = useMemo((): Map<string, PaletteCommand[]> => {
    const q = query.trim().toLowerCase();
    const groups = new Map<string, PaletteCommand[]>();

    if (!q) {
      // Recent group
      const recentSet = new Set(recentIds);
      const recentCmds = recentIds
        .map((id) => commands.find((c) => c.id === id))
        .filter((c): c is PaletteCommand => c !== undefined);
      if (recentCmds.length > 0) {
        groups.set('Recent', recentCmds);
      }
      // Remaining by category, using a consistent order
      const CATEGORY_ORDER = ['Navigation', 'Git', 'Search', 'Settings', 'Help'];
      const rest = commands.filter((c) => !recentSet.has(c.id));
      const byCategory = new Map<string, PaletteCommand[]>();
      for (const cmd of rest) {
        const existing = byCategory.get(cmd.category);
        if (existing) {
          existing.push(cmd);
        } else {
          byCategory.set(cmd.category, [cmd]);
        }
      }
      // Insert in preferred order, then any remaining categories
      for (const cat of CATEGORY_ORDER) {
        const cmds = byCategory.get(cat);
        if (cmds) groups.set(cat, cmds);
      }
      for (const [cat, cmds] of byCategory) {
        if (!groups.has(cat)) groups.set(cat, cmds);
      }
    } else {
      // Filtered: flat, no groups (but we still render one group)
      if (flatItems.length > 0) {
        groups.set('Results', flatItems);
      }
    }

    return groups;
  }, [commands, query, recentIds, flatItems]);

  useEffect(() => {
    if (isOpen) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setQuery('');
      setSelectedIndex(0);
      requestAnimationFrame(() => {
        inputRef.current?.focus();
      });
    }
  }, [isOpen]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSelectedIndex(0);
  }, [query]);

  const executeCommand = useCallback(
    (cmd: PaletteCommand) => {
      saveRecentId(cmd.id);
      close();
      cmd.action();
    },
    [close],
  );

  useEffect(() => {
    if (!isOpen) return;

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedIndex((prev) => (prev + 1) % Math.max(flatItems.length, 1));
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedIndex((prev) => (prev - 1 + flatItems.length) % Math.max(flatItems.length, 1));
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        const cmd = flatItems[selectedIndex];
        if (cmd) executeCommand(cmd);
        return;
      }
    }

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, close, flatItems, selectedIndex, executeCommand]);

  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-index="${selectedIndex}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex]);

  if (!isOpen) return null;

  // Build a flat index map from the grouped display order
  // so data-index aligns with flatItems
  const flatItemIds = flatItems.map((c) => c.id);

  function EntryTypeBadge({ cmd }: { cmd: PaletteCommand }) {
    if (cmd.type === 'file') {
      return (
        <span className="flex flex-shrink-0 items-center gap-0.5 rounded border border-border bg-navy-900 px-1.5 py-0.5 text-[10px] text-text-muted">
          <FileText size={9} />
          File
        </span>
      );
    }
    if (cmd.type === 'branch') {
      return (
        <span className="flex flex-shrink-0 items-center gap-0.5 rounded border border-border bg-navy-900 px-1.5 py-0.5 text-[10px] text-text-muted">
          <GitBranch size={9} />
          Branch
        </span>
      );
    }
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]">
      <div className="fixed inset-0 bg-navy-950/70" onClick={close} />
      <div className="relative z-10 w-full max-w-lg overflow-hidden rounded-xl border border-border bg-navy-800 shadow-2xl">
        {/* Search input */}
        <div className="flex items-center gap-2 border-b border-border px-4 py-3">
          <Search size={16} className="flex-shrink-0 text-text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Type a command or search..."
            aria-label="Command palette"
            className="flex-1 bg-transparent text-sm text-text-primary placeholder-text-muted focus:outline-none"
          />
          <kbd className="flex items-center gap-0.5 rounded border border-border bg-navy-900 px-1.5 py-0.5 text-[10px] text-text-muted">
            <Command size={10} />K
          </kbd>
        </div>

        {/* Results */}
        <div ref={listRef} className="max-h-80 overflow-y-auto py-1">
          {flatItems.length === 0 ? (
            <div className="px-4 py-6 text-center text-sm text-text-muted">No matching commands</div>
          ) : (
            Array.from(grouped.entries()).map(([category, cmds]) => {
              const isRecent = category === 'Recent';
              return (
                <div key={category}>
                  <div className="flex items-center gap-1.5 px-4 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
                    {isRecent && <Clock size={9} />}
                    {category}
                  </div>
                  {cmds.map((cmd) => {
                    const idx = flatItemIds.indexOf(cmd.id);
                    const isSelected = idx === selectedIndex;
                    return (
                      <button
                        key={cmd.id}
                        data-index={idx}
                        onClick={() => executeCommand(cmd)}
                        onMouseEnter={() => setSelectedIndex(idx)}
                        className={`flex w-full items-center gap-3 px-4 py-2 text-left text-sm transition-colors ${
                          isSelected
                            ? 'bg-accent/15 text-accent'
                            : 'text-text-secondary hover:bg-surface-hover'
                        }`}
                      >
                        <span className="flex-1">{cmd.label}</span>
                        <EntryTypeBadge cmd={cmd} />
                        {cmd.shortcut && (
                          <kbd className="flex-shrink-0 rounded border border-border bg-navy-900 px-1.5 py-0.5 text-[10px] text-text-muted">
                            {cmd.shortcut}
                          </kbd>
                        )}
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        {/* Footer hint */}
        <div className="flex items-center gap-3 border-t border-border px-4 py-2">
          <span className="text-[10px] text-text-muted">
            <kbd className="mr-0.5 rounded border border-border bg-navy-900 px-1 py-0.5 font-mono text-[9px]">↑↓</kbd>
            navigate
          </span>
          <span className="text-[10px] text-text-muted">
            <kbd className="mr-0.5 rounded border border-border bg-navy-900 px-1 py-0.5 font-mono text-[9px]">↵</kbd>
            execute
          </span>
          <span className="text-[10px] text-text-muted">
            <kbd className="mr-0.5 rounded border border-border bg-navy-900 px-1 py-0.5 font-mono text-[9px]">Esc</kbd>
            close
          </span>
        </div>
      </div>
    </div>
  );
}

export default CommandPalette;
