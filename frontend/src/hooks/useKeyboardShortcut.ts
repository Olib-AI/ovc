import { useEffect, useRef } from 'react';

interface KeyboardShortcutOptions {
  ctrl?: boolean;
  meta?: boolean;
  shift?: boolean;
  enabled?: boolean;
}

export function useKeyboardShortcut(
  key: string,
  callback: () => void,
  options?: KeyboardShortcutOptions,
): void {
  const callbackRef = useRef(callback);
  const optionsRef = useRef(options);

  useEffect(() => {
    callbackRef.current = callback;
    optionsRef.current = options;
  });

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const opts = optionsRef.current;
      if (opts?.enabled === false) return;

      const wantCtrl = opts?.ctrl ?? false;
      const wantMeta = opts?.meta ?? false;
      const wantShift = opts?.shift ?? false;

      // When `meta` is requested, accept either Cmd (macOS) or Ctrl (Windows/Linux)
      // so that shortcuts like Cmd+K work cross-platform without separate bindings.
      if (wantMeta) {
        if (!event.metaKey && !event.ctrlKey) return;
      } else {
        // meta not requested: ensure neither modifier fires unexpectedly
        if (wantCtrl !== event.ctrlKey) return;
        if (event.metaKey) return;
      }
      if (wantShift !== event.shiftKey) return;

      if (event.key.toLowerCase() !== key.toLowerCase()) return;

      // Skip when user is typing in an input field, unless it's a modifier combo
      const target = event.target;
      if (target instanceof HTMLElement) {
        const isInputField =
          target instanceof HTMLInputElement ||
          target instanceof HTMLTextAreaElement ||
          target.isContentEditable;
        const hasModifier = wantCtrl || wantMeta || event.ctrlKey || event.metaKey;
        if (isInputField && !hasModifier) return;
      }

      event.preventDefault();
      callbackRef.current();
    }

    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [key]);
}
