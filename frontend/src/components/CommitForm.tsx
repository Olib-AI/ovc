import { useState, useEffect, useCallback } from 'react';
import { Send, AlertTriangle, ShieldCheck, Loader2, Sparkles, Square } from 'lucide-react';
import { streamCommitMessage } from '../api/client.ts';
import { useLlmStream, useLlmConfig } from '../hooks/useLlm.ts';

interface CommitFormProps {
  stagedCount: number;
  onCommit: (
    message: string,
    authorName: string,
    authorEmail: string,
    options?: { amend?: boolean; sign?: boolean },
  ) => void;
  isCommitting: boolean;
  defaultAuthorName?: string;
  defaultAuthorEmail?: string;
  lastCommitMessage?: string;
  repoId?: string;
}

function CommitForm({
  stagedCount,
  onCommit,
  isCommitting,
  defaultAuthorName,
  defaultAuthorEmail,
  lastCommitMessage,
  repoId,
}: CommitFormProps) {
  const [message, setMessage] = useState('');
  const [authorName, setAuthorName] = useState(
    () => localStorage.getItem('ovc_author_name') ?? defaultAuthorName ?? 'User',
  );
  const [authorEmail, setAuthorEmail] = useState(
    () => localStorage.getItem('ovc_author_email') ?? defaultAuthorEmail ?? 'user@olib.ai',
  );
  // Update author name/email when async defaults arrive, but only if the user
  // hasn't already typed a custom value (i.e. it's still the initial placeholder).
  useEffect(() => {
    if (defaultAuthorName && authorName === 'User' && !localStorage.getItem('ovc_author_name')) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setAuthorName(defaultAuthorName);
    }
  }, [defaultAuthorName]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (defaultAuthorEmail && authorEmail === 'user@olib.ai' && !localStorage.getItem('ovc_author_email')) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setAuthorEmail(defaultAuthorEmail);
    }
  }, [defaultAuthorEmail]); // eslint-disable-line react-hooks/exhaustive-deps

  const [amend, setAmend] = useState(false);
  const [sign, setSign] = useState(false);

  // LLM commit message generation
  const { data: llmConfig } = useLlmConfig(repoId);
  const llmEnabled = !!repoId && (!!llmConfig?.server_enabled || !!llmConfig?.base_url) && (llmConfig?.enabled_features?.commit_message ?? true);

  const streamFn = useCallback(
    (signal: AbortSignal) => streamCommitMessage(repoId!, signal),
    [repoId],
  );
  const { content: aiContent, isStreaming: aiStreaming, error: aiError, progress: aiProgress, start: aiStart, cancel: aiCancel, reset: aiReset } = useLlmStream(streamFn);

  // Write streamed content into the message textarea.
  useEffect(() => {
    if (aiContent) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setMessage(aiContent);
    }
  }, [aiContent]);

  const handleAmendToggle = (checked: boolean) => {
    setAmend(checked);
    if (checked && message.trim() === '' && lastCommitMessage) {
      setMessage(lastCommitMessage);
    }
  };

  const firstLine = message.split('\n')[0] ?? '';
  const canCommit = (stagedCount > 0 || amend) && message.trim().length > 0 && !isCommitting;

  function handleCommit() {
    if (!canCommit) return;
    localStorage.setItem('ovc_author_name', authorName);
    localStorage.setItem('ovc_author_email', authorEmail);
    onCommit(message, authorName, authorEmail, {
      amend: amend || undefined,
      sign: sign || undefined,
    });
    setMessage('');
    setAmend(false);
    setSign(false);
  }

  return (
    <div className="flex-shrink-0 border-t border-border bg-navy-900 p-3">
      <div className="mb-2 space-y-1.5">
        <input
          value={authorName}
          onChange={(e) => setAuthorName(e.target.value)}
          placeholder="Author name"
          aria-label="Author name"
          className="w-full rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
        />
        <input
          value={authorEmail}
          onChange={(e) => setAuthorEmail(e.target.value)}
          placeholder="Author email"
          aria-label="Author email"
          title={authorEmail}
          className="w-full truncate rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
        />
      </div>

      <textarea
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            handleCommit();
          }
        }}
        placeholder="Commit message..."
        aria-label="Commit message"
        rows={3}
        className="w-full resize-none rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
      />

      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <label
          className={`flex cursor-pointer select-none items-center gap-1.5 rounded px-1.5 py-0.5 text-[11px] transition-colors ${
            amend
              ? 'border border-status-deleted/30 bg-status-deleted/10 text-status-deleted'
              : 'text-text-muted hover:text-text-secondary'
          }`}
          title="Amend the last commit instead of creating a new one"
        >
          <input
            type="checkbox"
            checked={amend}
            onChange={(e) => handleAmendToggle(e.target.checked)}
            className="sr-only"
          />
          <AlertTriangle size={11} />
          Amend
        </label>

        <label
          className={`flex cursor-pointer select-none items-center gap-1.5 rounded px-1.5 py-0.5 text-[11px] transition-colors ${
            sign
              ? 'border border-green-500/30 bg-green-500/10 text-green-400'
              : 'text-text-muted hover:text-text-secondary'
          }`}
          title="Sign commit with configured key"
        >
          <input
            type="checkbox"
            checked={sign}
            onChange={(e) => setSign(e.target.checked)}
            className="sr-only"
          />
          <ShieldCheck size={11} />
          Sign
        </label>

        {llmEnabled && (
          <button
            type="button"
            onClick={() => {
              if (aiStreaming) {
                aiCancel();
              } else {
                aiReset();
                aiStart();
              }
            }}
            disabled={stagedCount === 0 && !amend}
            title={aiStreaming ? 'Stop generating' : 'Generate commit message with AI'}
            className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors ${
              aiStreaming
                ? 'border border-status-deleted/30 bg-status-deleted/10 text-status-deleted'
                : 'text-accent hover:bg-accent/10'
            } disabled:opacity-40 disabled:cursor-not-allowed`}
          >
            {aiStreaming ? <Square size={10} /> : <Sparkles size={11} />}
            {aiStreaming ? 'Stop' : 'AI'}
          </button>
        )}

        {aiProgress && (
          <span className="text-[11px] text-accent animate-pulse">
            {aiProgress.phase === 'analyzing'
              ? `Analyzing ${aiProgress.batch}/${aiProgress.total}...`
              : 'Generating...'}
          </span>
        )}

        {aiError && (
          <span className="text-[11px] text-status-deleted" title={aiError}>AI error</span>
        )}

        <span className="text-[11px] text-text-muted">
          {firstLine.length}
        </span>

        <div className="ml-auto">
          <button
            onClick={handleCommit}
            disabled={!canCommit}
            aria-label="Commit changes"
            className={`flex items-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold transition-colors disabled:opacity-40 disabled:cursor-not-allowed ${
              amend
                ? 'bg-status-deleted text-white hover:opacity-90'
                : 'bg-accent text-navy-950 hover:bg-accent-light'
            }`}
          >
            {isCommitting ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <Send size={12} />
            )}
            {isCommitting
              ? 'Committing...'
              : `${amend ? 'Amend' : 'Commit'} (${stagedCount} file${stagedCount !== 1 ? 's' : ''})`}
          </button>
        </div>
      </div>
    </div>
  );
}

export default CommitForm;
