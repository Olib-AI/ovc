import { useState, useEffect, useRef } from 'react';
import axios from 'axios';
import { Play, ChevronDown, ChevronRight, RefreshCw, Zap, Container } from 'lucide-react';
import { useActionsList, useRunActions, useRunSingleAction, useInitActions } from '../hooks/useActions.ts';
import ActionCard from './ActionCard.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';
import type { ActionInfo, RunActionsResponse } from '../api/types.ts';

interface ActionsDashboardProps {
  repoId: string;
}

const CATEGORY_ORDER = ['lint', 'format', 'build', 'test', 'quality', 'security', 'audit', 'builtin', 'custom'] as const;

const CATEGORY_COLORS: Record<string, string> = {
  lint: 'text-blue-400',
  format: 'text-purple-400',
  build: 'text-orange-400',
  test: 'text-green-400',
  quality: 'text-teal-400',
  security: 'text-red-400',
  audit: 'text-red-400',
  builtin: 'text-cyan-400',
  custom: 'text-gray-400',
};

const CATEGORY_BG: Record<string, string> = {
  lint: 'bg-blue-500/10',
  format: 'bg-purple-500/10',
  build: 'bg-orange-500/10',
  test: 'bg-green-500/10',
  quality: 'bg-teal-500/10',
  security: 'bg-red-500/10',
  audit: 'bg-red-500/10',
  builtin: 'bg-cyan-500/10',
  custom: 'bg-gray-500/10',
};

function groupByCategory(actions: ActionInfo[]): Map<string, ActionInfo[]> {
  const groups = new Map<string, ActionInfo[]>();
  for (const action of actions) {
    const existing = groups.get(action.category);
    if (existing) {
      existing.push(action);
    } else {
      groups.set(action.category, [action]);
    }
  }
  return groups;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

const TRIGGER_OPTIONS = ['manual', 'pre-commit', 'post-commit', 'pre-push', 'pre-merge', 'post-merge', 'on-fail', 'schedule', 'pull-request'] as const;
type TriggerOption = typeof TRIGGER_OPTIONS[number];

function ActionsDashboard({ repoId }: ActionsDashboardProps) {
  const { data, isLoading, error } = useActionsList(repoId);
  const runAllMutation = useRunActions(repoId);
  const runSingleMutation = useRunSingleAction(repoId);
  const initMutation = useInitActions(repoId);
  const [collapsedCategories, setCollapsedCategories] = useState<Set<string>>(new Set());
  const [lastRunResult, setLastRunResult] = useState<RunActionsResponse | null>(null);
  const [runningAction, setRunningAction] = useState<string | null>(null);
  const [selectedTrigger, setSelectedTrigger] = useState<TriggerOption>('manual');
  const [showTriggerDropdown, setShowTriggerDropdown] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (triggerRef.current && !triggerRef.current.contains(e.target as Node)) {
        setShowTriggerDropdown(false);
      }
    }
    if (showTriggerDropdown) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [showTriggerDropdown]);

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading actions..." />;
  }

  if (error) {
    const isNotConfigured =
      (axios.isAxiosError(error) && error.response?.status === 404) ||
      (error instanceof Error &&
        (error.message.includes('404') || error.message.toLowerCase().includes('not found') || error.message.toLowerCase().includes('not configured')));

    if (isNotConfigured) {
      return (
        <div className="flex h-full flex-col items-center justify-center gap-4 p-8">
          <Zap size={40} className="text-text-muted" />
          <h2 className="text-lg font-semibold text-text-primary">No Actions Configured</h2>
          <p className="max-w-sm text-center text-sm text-text-muted">
            Initialize actions to automatically detect your project languages and set up linting,
            formatting, testing, and more.
          </p>
          <button
            onClick={() => initMutation.mutate(false)}
            disabled={initMutation.isPending}
            className="flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {initMutation.isPending ? (
              <RefreshCw size={14} className="animate-spin" />
            ) : (
              <Zap size={14} />
            )}
            Initialize Actions
          </button>
          {initMutation.isError && (
            <p className="text-xs text-red-400">
              Failed to initialize: {initMutation.error instanceof Error ? initMutation.error.message : 'Unknown error'}
            </p>
          )}
        </div>
      );
    }

    return <div className="p-8 text-red-400">Failed to load actions: {error.message}</div>;
  }

  const actions = data?.actions ?? [];

  if (actions.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 p-8">
        <Zap size={40} className="text-text-muted" />
        <h2 className="text-lg font-semibold text-text-primary">No Actions Configured</h2>
        <p className="max-w-sm text-center text-sm text-text-muted">
          Initialize actions to get started with automated code quality checks.
        </p>
        <button
          onClick={() => initMutation.mutate(false)}
          disabled={initMutation.isPending}
          className="flex items-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
        >
          {initMutation.isPending ? (
            <RefreshCw size={14} className="animate-spin" />
          ) : (
            <Zap size={14} />
          )}
          Initialize Actions
        </button>
      </div>
    );
  }

  const grouped = groupByCategory(actions);
  const sortedCategories = [...grouped.keys()].sort((a, b) => {
    const ai = CATEGORY_ORDER.indexOf(a as typeof CATEGORY_ORDER[number]);
    const bi = CATEGORY_ORDER.indexOf(b as typeof CATEGORY_ORDER[number]);
    return (ai === -1 ? 999 : ai) - (bi === -1 ? 999 : bi);
  });

  function toggleCategory(cat: string) {
    setCollapsedCategories((prev) => {
      const next = new Set(prev);
      if (next.has(cat)) {
        next.delete(cat);
      } else {
        next.add(cat);
      }
      return next;
    });
  }

  function handleRunAll() {
    runAllMutation.mutate(
      { trigger: selectedTrigger },
      {
        onSuccess: (result) => setLastRunResult(result),
      },
    );
  }

  function handleRunSingle(name: string) {
    setRunningAction(name);
    runSingleMutation.mutate(
      { name },
      {
        onSettled: () => setRunningAction(null),
        onSuccess: (result) => setLastRunResult(result),
      },
    );
  }

  function handleFixSingle(name: string) {
    setRunningAction(name);
    runSingleMutation.mutate(
      { name, fix: true },
      {
        onSettled: () => setRunningAction(null),
        onSuccess: (result) => setLastRunResult(result),
      },
    );
  }

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs text-text-muted">{actions.length} action{actions.length !== 1 ? 's' : ''}</span>
        </div>
        <div className="relative" ref={triggerRef}>
          <div className="flex">
            <button
              onClick={handleRunAll}
              disabled={runAllMutation.isPending}
              className="flex items-center gap-1.5 rounded-l-lg bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
            >
              {runAllMutation.isPending ? (
                <RefreshCw size={13} className="animate-spin" />
              ) : (
                <Play size={13} />
              )}
              Run All
              {selectedTrigger !== 'manual' && (
                <span className="rounded bg-navy-950/15 px-1 py-0.5 text-[9px] font-bold uppercase">
                  {selectedTrigger}
                </span>
              )}
            </button>
            <button
              onClick={() => setShowTriggerDropdown((v) => !v)}
              disabled={runAllMutation.isPending}
              className="flex items-center rounded-r-lg border-l border-navy-950/20 bg-accent px-1.5 py-1.5 text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
              aria-label="Select trigger type"
            >
              <ChevronDown size={13} />
            </button>
          </div>
          {showTriggerDropdown && (
            <div className="absolute right-0 top-full z-30 mt-1 w-44 overflow-hidden rounded-md border border-border bg-navy-800 shadow-lg">
              {TRIGGER_OPTIONS.map((trigger) => (
                <button
                  key={trigger}
                  onClick={() => {
                    setSelectedTrigger(trigger);
                    setShowTriggerDropdown(false);
                  }}
                  className={`flex w-full px-3 py-2 text-left text-xs transition-colors hover:bg-surface-hover ${
                    selectedTrigger === trigger ? 'bg-accent/10 text-accent' : 'text-text-primary'
                  }`}
                >
                  {trigger}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {lastRunResult && (
        <div
          className={`mb-4 rounded-lg border p-3 text-xs ${
            lastRunResult.overall_status === 'passed'
              ? 'border-green-500/30 bg-green-500/5'
              : 'border-red-500/30 bg-red-500/5'
          }`}
        >
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <div className="flex items-center gap-2">
              <span className={`font-semibold ${lastRunResult.overall_status === 'passed' ? 'text-green-300' : 'text-red-300'}`}>
                {lastRunResult.overall_status === 'passed' ? 'All checks passed' : 'Some checks failed'}
              </span>
              {/* Trigger badge for the last run */}
              <span className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
                lastRunResult.trigger === 'manual'
                  ? 'bg-gray-500/20 text-gray-300'
                  : lastRunResult.trigger === 'pre-commit'
                    ? 'bg-yellow-500/20 text-yellow-300'
                    : lastRunResult.trigger === 'pre-push'
                      ? 'bg-blue-500/20 text-blue-300'
                      : lastRunResult.trigger === 'schedule'
                        ? 'bg-teal-500/20 text-teal-300'
                        : lastRunResult.trigger === 'pull-request'
                          ? 'bg-cyan-500/20 text-cyan-300'
                          : 'bg-purple-500/20 text-purple-300'
              }`}>
                {lastRunResult.trigger}
              </span>
            </div>
            <span className="text-text-muted">
              {lastRunResult.results.length} action{lastRunResult.results.length !== 1 ? 's' : ''} · {formatDuration(lastRunResult.total_duration_ms)}
            </span>
          </div>

          {/* Per-action status summary */}
          <div className="mt-2 flex flex-wrap gap-1">
            {lastRunResult.results.map((r) => (
              <span
                key={r.name}
                title={r.display_name}
                className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                  r.status === 'passed' ? 'bg-green-500/15 text-green-400' : 'bg-red-500/15 text-red-400'
                }`}
              >
                {r.status === 'passed' ? '✓' : '✗'} {r.display_name}
              </span>
            ))}
          </div>

          {lastRunResult.results.some((r) => r.status === 'failed') && (
            <div className="mt-2 space-y-1">
              {lastRunResult.results
                .filter((r) => r.status === 'failed')
                .map((r) => (
                  <div key={r.name} className="rounded bg-navy-950 p-2">
                    <div className="flex items-center gap-1.5 font-medium text-red-400">
                      {r.display_name}
                      {r.docker_used && (
                        <span className="inline-flex items-center gap-0.5 rounded bg-cyan-500/15 px-1 py-0.5 text-[9px] font-medium text-cyan-300" title="Ran in Docker">
                          <Container size={9} />
                          docker
                        </span>
                      )}
                    </div>
                    {r.stderr && (
                      <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">
                        {r.stderr}
                      </pre>
                    )}
                    {r.stdout && !r.stderr && (
                      <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] text-text-secondary">
                        {r.stdout}
                      </pre>
                    )}
                  </div>
                ))}
            </div>
          )}
        </div>
      )}

      <div className="space-y-3">
        {sortedCategories.map((category) => {
          const catActions = grouped.get(category);
          if (!catActions) return null;
          const isCollapsed = collapsedCategories.has(category);
          const catColor = CATEGORY_COLORS[category] ?? 'text-gray-400';
          const catBg = CATEGORY_BG[category] ?? 'bg-gray-500/10';

          return (
            <div key={category} className={`rounded-lg border border-border ${catBg}`}>
              <button
                onClick={() => toggleCategory(category)}
                className="flex w-full items-center gap-2 px-4 py-2.5"
              >
                {isCollapsed ? (
                  <ChevronRight size={14} className="text-text-muted" />
                ) : (
                  <ChevronDown size={14} className="text-text-muted" />
                )}
                <span className={`text-xs font-semibold uppercase tracking-wider ${catColor}`}>
                  {category}
                </span>
                <span className="text-[10px] text-text-muted">({catActions.length})</span>
              </button>

              {!isCollapsed && (
                <div className="space-y-2 px-3 pb-3">
                  {catActions.map((action) => (
                    <ActionCard
                      key={action.name}
                      action={action}
                      onRun={handleRunSingle}
                      onFix={handleFixSingle}
                      isRunning={
                        runningAction === action.name ||
                        (runAllMutation.isPending && !runningAction)
                      }
                    />
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export default ActionsDashboard;
