import { useState } from 'react';
import { ChevronDown, ChevronRight, Loader2, Sparkles, Square, Copy, Check } from 'lucide-react';

interface LlmPanelProps {
  /** Panel title shown in the header. */
  title: string;
  /** Accumulated streaming content from the LLM. */
  content: string;
  /** Whether the LLM is currently streaming. */
  isStreaming: boolean;
  /** Error message, if any. */
  error: string | null;
  /** Callback to start generation. */
  onGenerate: () => void;
  /** Callback to cancel the active stream. */
  onCancel: () => void;
  /** Optional callback to insert the generated text elsewhere. */
  onInsert?: (text: string) => void;
  /** Label for the insert button (default: "Use this"). */
  insertLabel?: string;
  /** Whether the generate button is disabled. */
  disabled?: boolean;
}

function LlmPanel({
  title,
  content,
  isStreaming,
  error,
  onGenerate,
  onCancel,
  onInsert,
  insertLabel = 'Use this',
  disabled = false,
}: LlmPanelProps) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);

  const hasContent = content.length > 0;
  const showPanel = expanded || isStreaming || hasContent;

  const handleCopy = () => {
    void navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="rounded border border-border bg-navy-900/50">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-2 px-3 py-2 text-xs font-medium text-text-secondary hover:text-text-primary transition-colors"
      >
        {showPanel ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Sparkles size={12} className="text-accent" />
        {title}
        {isStreaming && <Loader2 size={12} className="ml-auto animate-spin text-accent" />}
      </button>

      {showPanel && (
        <div className="border-t border-border px-3 pb-3 pt-2">
          {!hasContent && !isStreaming && !error && (
            <button
              type="button"
              onClick={onGenerate}
              disabled={disabled}
              className="flex items-center gap-1.5 rounded bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent hover:bg-accent/20 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <Sparkles size={12} />
              Generate
            </button>
          )}

          {isStreaming && (
            <button
              type="button"
              onClick={onCancel}
              className="flex items-center gap-1.5 rounded bg-status-deleted/10 px-3 py-1.5 text-xs font-medium text-status-deleted hover:bg-status-deleted/20 transition-colors"
            >
              <Square size={10} />
              Stop
            </button>
          )}

          {error && (
            <p className="text-xs text-status-deleted">{error}</p>
          )}

          {hasContent && (
            <div className="mt-2">
              <pre className="max-h-60 overflow-auto whitespace-pre-wrap rounded bg-navy-950 p-3 text-xs text-text-primary font-mono leading-relaxed">
                {content}
              </pre>
              <div className="mt-2 flex items-center gap-2">
                {onInsert && !isStreaming && (
                  <button
                    type="button"
                    onClick={() => onInsert(content)}
                    className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-semibold text-navy-950 hover:bg-accent-light transition-colors"
                  >
                    {insertLabel}
                  </button>
                )}
                <button
                  type="button"
                  onClick={handleCopy}
                  className="flex items-center gap-1 rounded px-2 py-1 text-xs text-text-muted hover:text-text-secondary transition-colors"
                >
                  {copied ? <Check size={12} /> : <Copy size={12} />}
                  {copied ? 'Copied' : 'Copy'}
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default LlmPanel;
