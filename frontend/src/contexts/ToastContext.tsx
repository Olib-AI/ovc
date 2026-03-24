/* eslint-disable react-refresh/only-export-components */
import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';

type ToastType = 'success' | 'error' | 'info' | 'warning' | 'progress';

interface ToastAction {
  label: string;
  onClick: () => void;
}

interface Toast {
  id: string;
  type: ToastType;
  message: string;
  action?: ToastAction;
}

interface ToastContextValue {
  toasts: Toast[];
  success: (message: string, action?: ToastAction) => void;
  error: (message: string, action?: ToastAction) => void;
  info: (message: string, action?: ToastAction) => void;
  warning: (message: string, action?: ToastAction) => void;
  progress: (message: string) => string;
  updateToast: (id: string, type: ToastType, message: string, action?: ToastAction) => void;
  dismiss: (id: string) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

const MAX_TOASTS = 5;
const AUTO_DISMISS_MS = 4000;

function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const timersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
    const timer = timersRef.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timersRef.current.delete(id);
    }
  }, []);

  const addToast = useCallback(
    (type: ToastType, message: string, action?: ToastAction) => {
      const id = crypto.randomUUID();
      setToasts((prev) => {
        const next = [...prev, { id, type, message, action }];
        if (next.length > MAX_TOASTS) {
          const removed = next.shift();
          if (removed) {
            const timer = timersRef.current.get(removed.id);
            if (timer !== undefined) {
              clearTimeout(timer);
              timersRef.current.delete(removed.id);
            }
          }
        }
        return next;
      });
      if (type !== 'progress') {
        const timer = setTimeout(() => {
          dismiss(id);
        }, AUTO_DISMISS_MS);
        timersRef.current.set(id, timer);
      }
      return id;
    },
    [dismiss],
  );

  const updateToast = useCallback(
    (id: string, type: ToastType, message: string, action?: ToastAction) => {
      setToasts((prev) =>
        prev.map((t) => (t.id === id ? { ...t, type, message, action } : t)),
      );
      // If transitioning from progress to a final state, auto-dismiss
      if (type !== 'progress') {
        const existingTimer = timersRef.current.get(id);
        if (existingTimer !== undefined) {
          clearTimeout(existingTimer);
        }
        const timer = setTimeout(() => {
          dismiss(id);
        }, AUTO_DISMISS_MS);
        timersRef.current.set(id, timer);
      }
    },
    [dismiss],
  );

  useEffect(() => {
    const timers = timersRef.current;
    return () => {
      timers.forEach((timer) => clearTimeout(timer));
      timers.clear();
    };
  }, []);

  const success = useCallback(
    (message: string, action?: ToastAction) => { addToast('success', message, action); },
    [addToast],
  );
  const error = useCallback(
    (message: string, action?: ToastAction) => { addToast('error', message, action); },
    [addToast],
  );
  const info = useCallback(
    (message: string, action?: ToastAction) => { addToast('info', message, action); },
    [addToast],
  );
  const warning = useCallback(
    (message: string, action?: ToastAction) => { addToast('warning', message, action); },
    [addToast],
  );
  const progress = useCallback(
    (message: string) => addToast('progress', message),
    [addToast],
  );

  return (
    <ToastContext.Provider value={{ toasts, success, error, info, warning, progress, updateToast, dismiss }}>
      {children}
    </ToastContext.Provider>
  );
}

function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) {
    throw new Error('useToast must be used within a ToastProvider');
  }
  return ctx;
}

export { ToastProvider, useToast };
export type { Toast, ToastType, ToastAction };
