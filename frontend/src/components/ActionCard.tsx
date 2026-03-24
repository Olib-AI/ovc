import { Play, Wrench, Clock, CheckCircle2, XCircle, Loader2 } from 'lucide-react';
import { useSearchParams } from 'react-router-dom';
import type { ActionInfo } from '../api/types.ts';

interface ActionCardProps {
  action: ActionInfo;
  onRun: (name: string) => void;
  onFix: (name: string) => void;
  isRunning: boolean;
}

const LANGUAGE_COLORS: Record<string, string> = {
  rust: 'bg-orange-500/20 text-orange-300',
  javascript: 'bg-yellow-500/20 text-yellow-300',
  typescript: 'bg-blue-500/20 text-blue-300',
  python: 'bg-blue-600/20 text-blue-300',
  go: 'bg-cyan-500/20 text-cyan-300',
  ruby: 'bg-red-500/20 text-red-300',
  java: 'bg-red-600/20 text-red-400',
  c: 'bg-gray-500/20 text-gray-300',
  cpp: 'bg-purple-500/20 text-purple-300',
  csharp: 'bg-green-500/20 text-green-300',
  swift: 'bg-orange-600/20 text-orange-300',
  kotlin: 'bg-violet-500/20 text-violet-300',
  php: 'bg-indigo-500/20 text-indigo-300',
  elixir: 'bg-purple-600/20 text-purple-300',
  'c#': 'bg-green-500/20 text-green-300',
  shell: 'bg-green-600/20 text-green-300',
  dart: 'bg-sky-500/20 text-sky-300',
  deno: 'bg-lime-500/20 text-lime-300',
};

const TRIGGER_COLORS: Record<string, string> = {
  manual: 'bg-gray-500/20 text-gray-300',
  'pre-commit': 'bg-yellow-500/20 text-yellow-300',
  'post-commit': 'bg-yellow-600/20 text-yellow-200',
  'pre-push': 'bg-blue-500/20 text-blue-300',
  'pre-merge': 'bg-indigo-500/20 text-indigo-300',
  'post-merge': 'bg-indigo-600/20 text-indigo-200',
  'on-fail': 'bg-red-500/20 text-red-300',
  schedule: 'bg-teal-500/20 text-teal-300',
  ci: 'bg-purple-500/20 text-purple-300',
  'pull-request': 'bg-cyan-500/20 text-cyan-300',
};

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function formatTimestamp(ts: string): string {
  const date = new Date(ts);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
}

/** Status indicator shown at the left edge of the card. */
function StatusIndicator({ action, isRunning }: { action: ActionInfo; isRunning: boolean }) {
  if (isRunning) {
    return <Loader2 size={14} className="flex-shrink-0 animate-spin text-yellow-400" />;
  }
  if (!action.last_run) {
    return <div className="h-2.5 w-2.5 flex-shrink-0 rounded-full bg-gray-600" />;
  }
  if (action.last_run.status === 'passed') {
    return <CheckCircle2 size={14} className="flex-shrink-0 text-green-400" />;
  }
  if (action.last_run.status === 'failed') {
    return <XCircle size={14} className="flex-shrink-0 text-red-400" />;
  }
  return <div className="h-2.5 w-2.5 flex-shrink-0 rounded-full bg-gray-500" />;
}

/** Compact last-run status strip shown below the action header. */
function LastRunStrip({ action, isRunning }: { action: ActionInfo; isRunning: boolean }) {
  if (isRunning) {
    return (
      <div className="mt-2 flex items-center gap-2 text-[10px] text-yellow-400">
        <Loader2 size={10} className="animate-spin" />
        <span className="font-medium">Running…</span>
      </div>
    );
  }
  if (!action.last_run) return null;

  const { status, duration_ms, timestamp } = action.last_run;
  const statusColor =
    status === 'passed' ? 'text-green-400' : status === 'failed' ? 'text-red-400' : 'text-gray-400';

  return (
    <div className="mt-2 flex items-center gap-3 text-[10px] text-text-muted">
      <span className={`font-semibold ${statusColor}`}>{status}</span>
      <span>{formatDuration(duration_ms)}</span>
      <span>{formatTimestamp(timestamp)}</span>
    </div>
  );
}

function ActionCard({ action, onRun, onFix, isRunning }: ActionCardProps) {
  const [, setSearchParams] = useSearchParams();
  const canFix = action.category === 'format' || action.category === 'lint';
  const langKey = action.language?.toLowerCase();
  const langColor =
    langKey && langKey in LANGUAGE_COLORS
      ? LANGUAGE_COLORS[langKey]
      : 'bg-gray-500/20 text-gray-300';

  return (
    <div
      className={`rounded-lg border bg-navy-800 p-3 transition-colors ${
        isRunning ? 'border-yellow-500/30' : 'border-border'
      }`}
    >
      <div className="flex items-center gap-3">
        <StatusIndicator action={action} isRunning={isRunning} />

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-medium text-text-primary">
              {action.display_name}
            </span>
            {action.language && (
              <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${langColor}`}>
                {action.language}
              </span>
            )}
            {action.tool && (
              <span className="text-[10px] text-text-muted">{action.tool}</span>
            )}
          </div>

          {/* Trigger badges */}
          {action.triggers.length > 0 && (
            <div className="mt-1 flex flex-wrap items-center gap-1">
              {action.triggers.map((trigger) => (
                <span
                  key={trigger}
                  className={`rounded-full px-1.5 py-0 text-[9px] font-medium leading-4 ${TRIGGER_COLORS[trigger] ?? 'bg-gray-500/20 text-gray-300'}`}
                >
                  {trigger}
                </span>
              ))}
            </div>
          )}
        </div>

        <div className="flex flex-shrink-0 items-center gap-1.5">
          {canFix && (
            <button
              onClick={() => onFix(action.name)}
              disabled={isRunning}
              className="flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-purple-400/40 hover:text-purple-300 disabled:opacity-50"
              title="Run with --fix"
            >
              <Wrench size={11} />
              Fix
            </button>
          )}
          <button
            onClick={() => onRun(action.name)}
            disabled={isRunning}
            className="flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-accent disabled:opacity-50"
          >
            {isRunning ? <Loader2 size={11} className="animate-spin" /> : <Play size={11} />}
            {isRunning ? 'Running' : 'Run'}
          </button>
          {action.last_run && (
            <button
              onClick={() => setSearchParams({ tab: 'history' }, { replace: true })}
              className="flex items-center gap-1 rounded p-1 text-[10px] text-text-muted transition-colors hover:text-accent"
              title="View in History"
            >
              <Clock size={11} />
            </button>
          )}
        </div>
      </div>

      <LastRunStrip action={action} isRunning={isRunning} />
    </div>
  );
}

export default ActionCard;
