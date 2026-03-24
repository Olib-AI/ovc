import { useToast } from '../contexts/ToastContext.tsx';
import type { ToastType } from '../contexts/ToastContext.tsx';
import { CheckCircle2, XCircle, Info, AlertTriangle, X, Loader2 } from 'lucide-react';

const ICON_MAP: Record<ToastType, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
  warning: AlertTriangle,
  progress: Loader2,
};

const BORDER_COLOR_MAP: Record<ToastType, string> = {
  success: 'border-l-status-added',
  error: 'border-l-status-deleted',
  info: 'border-l-blue-500',
  warning: 'border-l-yellow-500',
  progress: 'border-l-accent',
};

const ICON_COLOR_MAP: Record<ToastType, string> = {
  success: 'text-status-added',
  error: 'text-status-deleted',
  info: 'text-blue-400',
  warning: 'text-yellow-400',
  progress: 'text-accent',
};

function ToastContainer() {
  const { toasts, dismiss } = useToast();

  if (toasts.length === 0) return null;

  return (
    // aria-live="polite" announces new toasts to screen readers without interrupting
    // the current read flow. Error toasts additionally carry role="alert" on the
    // individual item, which triggers an assertive announcement regardless of the
    // container's politeness setting.
    <div
      aria-live="polite"
      aria-label="Notifications"
      className="fixed right-4 bottom-4 z-50 flex flex-col gap-2"
    >
      {toasts.map((toast) => {
        const Icon = ICON_MAP[toast.type];
        const isError = toast.type === 'error';
        const isProgress = toast.type === 'progress';
        return (
          <div
            key={toast.id}
            role={isError ? 'alert' : undefined}
            className={`flex w-80 items-start gap-2.5 rounded-md border border-border border-l-4 ${BORDER_COLOR_MAP[toast.type]} bg-navy-800 px-3 py-2.5 shadow-lg animate-[slideIn_200ms_ease-out]`}
          >
            <Icon
              size={16}
              className={`mt-0.5 flex-shrink-0 ${ICON_COLOR_MAP[toast.type]} ${isProgress ? 'animate-spin' : ''}`}
            />
            <div className="min-w-0 flex-1">
              <p className="text-xs text-text-primary">{toast.message}</p>
              {toast.action && (
                <button
                  onClick={() => {
                    toast.action?.onClick();
                    dismiss(toast.id);
                  }}
                  className="mt-1 text-xs font-medium text-accent transition-colors hover:text-accent-light"
                >
                  {toast.action.label}
                </button>
              )}
            </div>
            {/* Progress toasts are now dismissible so a hung operation never
                leaves an undismissable spinner on screen */}
            <button
              onClick={() => dismiss(toast.id)}
              className="flex-shrink-0 rounded p-0.5 text-text-muted transition-colors hover:text-text-primary"
              aria-label="Dismiss notification"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

export default ToastContainer;
