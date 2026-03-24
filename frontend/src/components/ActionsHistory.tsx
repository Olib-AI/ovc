import { useState } from 'react';
import axios from 'axios';
import { ChevronDown, ChevronRight, Trash2, Container, CheckCircle2, XCircle, BarChart3 } from 'lucide-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useActionsHistory, useActionRun } from '../hooks/useActions.ts';
import { clearActionsHistory } from '../api/client.ts';
import LoadingSpinner from './LoadingSpinner.tsx';
import type { ActionRunSummary } from '../api/types.ts';

interface ActionsHistoryProps {
  repoId: string;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function formatTimestamp(ts: string): string {
  return new Date(ts).toLocaleString();
}

function shortenRunId(runId: string): string {
  return runId.length > 8 ? runId.slice(0, 8) : runId;
}

const TRIGGER_BADGE: Record<string, string> = {
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

function RunRow({ run, repoId }: { run: ActionRunSummary; repoId: string }) {
  const [expanded, setExpanded] = useState(false);
  const { data: runDetail } = useActionRun(repoId, expanded ? run.run_id : null);
  const [expandedResults, setExpandedResults] = useState<Set<string>>(new Set());

  function toggleResult(name: string) {
    setExpandedResults((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  const isPassed = run.overall_status === 'passed';

  return (
    <div className="border-b border-border last:border-b-0">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-surface-hover"
      >
        {expanded ? (
          <ChevronDown size={13} className="flex-shrink-0 text-text-muted" />
        ) : (
          <ChevronRight size={13} className="flex-shrink-0 text-text-muted" />
        )}
        <span className="w-20 flex-shrink-0 font-mono text-[11px] text-text-muted">
          {shortenRunId(run.run_id)}
        </span>
        <span
          className={`w-24 flex-shrink-0 truncate rounded-full px-2 py-0.5 text-center text-[10px] font-medium ${TRIGGER_BADGE[run.trigger] ?? 'bg-purple-500/20 text-purple-300'}`}
        >
          {run.trigger}
        </span>
        {/* Status with icon */}
        <span className={`flex w-20 flex-shrink-0 items-center justify-center gap-1 text-[11px] font-semibold ${isPassed ? 'text-green-400' : 'text-red-400'}`}>
          {isPassed
            ? <CheckCircle2 size={12} className="flex-shrink-0" />
            : <XCircle size={12} className="flex-shrink-0" />}
          {run.overall_status}
        </span>
        {/* Pass/fail fraction with mini progress bar */}
        <span className="flex w-24 flex-shrink-0 items-center gap-1.5">
          <span className="text-[10px] text-text-secondary">{run.passed_count}/{run.action_count}</span>
          {run.action_count > 0 && (
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-navy-700">
              <div
                className={`h-full rounded-full ${isPassed ? 'bg-green-400' : 'bg-red-400'}`}
                style={{ width: `${(run.passed_count / run.action_count) * 100}%` }}
              />
            </div>
          )}
        </span>
        <span className="w-16 flex-shrink-0 text-right text-[11px] text-text-muted">
          {formatDuration(run.total_duration_ms)}
        </span>
        <span className="flex-1 text-right text-[10px] text-text-muted">
          {formatTimestamp(run.timestamp)}
        </span>
      </button>

      {expanded && (
        <div className="bg-navy-950/50 px-4 py-2">
          {!runDetail ? (
            <LoadingSpinner size={14} className="py-2" />
          ) : (
            <div className="space-y-1.5">
              {runDetail.results.map((result) => (
                <div key={result.name} className="rounded border border-border bg-navy-900 text-xs">
                  <button
                    onClick={() => toggleResult(result.name)}
                    className="flex w-full items-center gap-2 px-3 py-2 text-left"
                  >
                    <div
                      className={`h-2 w-2 flex-shrink-0 rounded-full ${
                        result.status === 'passed' ? 'bg-green-400' : 'bg-red-400'
                      }`}
                    />
                    <span className="flex-1 text-text-primary">{result.display_name}</span>
                    {result.docker_used && (
                      <span className="inline-flex items-center gap-0.5 rounded bg-cyan-500/15 px-1 py-0.5 text-[9px] font-medium text-cyan-300" title="Ran in Docker">
                        <Container size={9} />
                        docker
                      </span>
                    )}
                    <span className="text-[10px] text-text-muted">
                      {formatDuration(result.duration_ms)}
                    </span>
                    {expandedResults.has(result.name) ? (
                      <ChevronDown size={12} className="text-text-muted" />
                    ) : (
                      <ChevronRight size={12} className="text-text-muted" />
                    )}
                  </button>

                  {expandedResults.has(result.name) && (result.stdout || result.stderr) && (
                    <div className="border-t border-border px-3 py-2">
                      {result.stdout && (
                        <div className="mb-2">
                          <div className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-text-muted">
                            stdout
                          </div>
                          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded bg-navy-950 p-2 font-mono text-[10px] text-text-secondary">
                            {result.stdout}
                          </pre>
                        </div>
                      )}
                      {result.stderr && (
                        <div>
                          <div className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-text-muted">
                            stderr
                          </div>
                          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded bg-navy-950 p-2 font-mono text-[10px] text-red-300/80">
                            {result.stderr}
                          </pre>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ActionsHistory({ repoId }: ActionsHistoryProps) {
  const [limit, setLimit] = useState(20);
  const { data, isLoading, error } = useActionsHistory(repoId, limit);
  const queryClient = useQueryClient();
  const clearMutation = useMutation({
    mutationFn: () => clearActionsHistory(repoId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['repo', repoId, 'actions-history'] });
    },
  });

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading history..." />;
  }

  if (error) {
    const isNotConfigured =
      (axios.isAxiosError(error) && error.response?.status === 404) ||
      (error instanceof Error &&
        (error.message.includes('404') || error.message.toLowerCase().includes('not found') || error.message.toLowerCase().includes('not configured')));

    if (isNotConfigured) {
      return (
        <div className="flex h-full items-center justify-center text-text-muted">
          <p className="text-sm">No actions configured yet. Initialize actions from the Dashboard tab.</p>
        </div>
      );
    }

    return <div className="p-8 text-red-400">Failed to load history: {error.message}</div>;
  }

  const runs = data?.runs ?? [];

  if (runs.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-text-muted">
        <p className="text-sm">No action runs yet</p>
      </div>
    );
  }

  const totalRuns = runs.length;
  const passedRuns = runs.filter((r) => r.overall_status === 'passed').length;
  const failedRuns = totalRuns - passedRuns;
  const successRate = totalRuns > 0 ? Math.round((passedRuns / totalRuns) * 100) : 0;

  return (
    <div className="h-full overflow-y-auto">
      {/* Summary stats strip */}
      <div className="flex items-center gap-4 border-b border-border bg-navy-950/50 px-4 py-2.5">
        <BarChart3 size={13} className="flex-shrink-0 text-text-muted" />
        <div className="flex flex-1 items-center gap-4 text-[11px]">
          <span className="text-text-muted">{totalRuns} run{totalRuns !== 1 ? 's' : ''}</span>
          <span className="text-green-400 font-medium">{passedRuns} passed</span>
          {failedRuns > 0 && <span className="text-red-400 font-medium">{failedRuns} failed</span>}
          <div className="flex items-center gap-1.5">
            <div className="h-1.5 w-24 overflow-hidden rounded-full bg-navy-700">
              <div className="h-full rounded-full bg-green-400" style={{ width: `${successRate}%` }} />
            </div>
            <span className="text-text-muted">{successRate}% success</span>
          </div>
        </div>
      </div>

      <div className="sticky top-0 flex items-center gap-3 border-b border-border bg-navy-900 px-4 py-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
        <span className="w-5" />
        <span className="w-20">Run ID</span>
        <span className="w-24">Trigger</span>
        <span className="w-20 text-center">Status</span>
        <span className="w-24">Pass Rate</span>
        <span className="w-16 text-right">Duration</span>
        <span className="flex-1 text-right">
          <button
            onClick={() => clearMutation.mutate()}
            disabled={clearMutation.isPending}
            className="inline-flex items-center gap-1 rounded px-2 py-1 text-[10px] normal-case tracking-normal text-red-400 transition-colors hover:bg-red-400/10 disabled:opacity-50"
            title="Clear all history"
          >
            <Trash2 size={11} />
            {clearMutation.isPending ? 'Clearing...' : 'Clear'}
          </button>
        </span>
      </div>

      {runs.map((run) => (
        <RunRow key={run.run_id} run={run} repoId={repoId} />
      ))}

      {runs.length >= limit && (
        <div className="p-3 text-center">
          <button
            onClick={() => setLimit((prev) => prev + 20)}
            className="rounded px-3 py-1.5 text-xs text-accent transition-colors hover:bg-accent/10"
          >
            Load more
          </button>
        </div>
      )}
    </div>
  );
}

export default ActionsHistory;
