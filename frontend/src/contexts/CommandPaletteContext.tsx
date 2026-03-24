/* eslint-disable react-refresh/only-export-components */
import { createContext, useCallback, useContext, useMemo, useState } from 'react';
import type { ReactNode } from 'react';

export interface PaletteCommand {
  id: string;
  label: string;
  category: string;
  shortcut?: string;
  type?: 'command' | 'file' | 'branch';
  action: () => void;
}

interface CommandPaletteContextValue {
  isOpen: boolean;
  open: () => void;
  close: () => void;
  commands: PaletteCommand[];
  registerCommands: (commands: PaletteCommand[]) => void;
  unregisterCommands: (ids: string[]) => void;
}

const CommandPaletteContext = createContext<CommandPaletteContextValue | null>(null);

function CommandPaletteProvider({ children }: { children: ReactNode }) {
  const [isOpen, setIsOpen] = useState(false);
  const [commandMap, setCommandMap] = useState<Map<string, PaletteCommand>>(new Map());

  const open = useCallback(() => setIsOpen(true), []);
  const close = useCallback(() => setIsOpen(false), []);

  const registerCommands = useCallback((cmds: PaletteCommand[]) => {
    setCommandMap((prev) => {
      const next = new Map(prev);
      for (const cmd of cmds) {
        next.set(cmd.id, cmd);
      }
      return next;
    });
  }, []);

  const unregisterCommands = useCallback((ids: string[]) => {
    setCommandMap((prev) => {
      const next = new Map(prev);
      for (const id of ids) {
        next.delete(id);
      }
      return next;
    });
  }, []);

  const commands = useMemo(() => Array.from(commandMap.values()), [commandMap]);

  const value = useMemo<CommandPaletteContextValue>(
    () => ({ isOpen, open, close, commands, registerCommands, unregisterCommands }),
    [isOpen, open, close, commands, registerCommands, unregisterCommands],
  );

  return (
    <CommandPaletteContext.Provider value={value}>{children}</CommandPaletteContext.Provider>
  );
}

function useCommandPalette(): CommandPaletteContextValue {
  const ctx = useContext(CommandPaletteContext);
  if (!ctx) {
    throw new Error('useCommandPalette must be used within a CommandPaletteProvider');
  }
  return ctx;
}

export { CommandPaletteProvider, useCommandPalette };
