import type { ReactNode } from 'react';

/** Shimmer-animated block for skeleton loading states. */
function SkeletonBlock({ className = '' }: { className?: string }) {
  return (
    <div
      className={`skeleton-shimmer rounded ${className}`}
      aria-hidden="true"
    />
  );
}

/** A full-height page-level skeleton for list pages. */
function RepoListSkeleton() {
  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mb-6 flex items-start justify-between">
        <div className="space-y-2">
          <SkeletonBlock className="h-6 w-36" />
          <SkeletonBlock className="h-4 w-56" />
        </div>
        <SkeletonBlock className="h-8 w-32" />
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="rounded-lg border border-border bg-navy-900 p-4">
            <div className="flex items-center gap-2">
              <SkeletonBlock className="h-8 w-8 rounded-md" />
              <div className="space-y-1.5">
                <SkeletonBlock className="h-4 w-28" />
                <SkeletonBlock className="h-3 w-20" />
              </div>
            </div>
            <div className="mt-3 flex gap-3">
              {Array.from({ length: 4 }).map((_, j) => (
                <SkeletonBlock key={j} className="h-3 w-10" />
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Skeleton for a commit list (history page graph area) */
function CommitListSkeleton({ rows = 12 }: { rows?: number }) {
  return (
    <div className="space-y-0 divide-y divide-border/30">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="flex items-center gap-3 px-4 py-2.5">
          <SkeletonBlock className="h-3 w-3 flex-shrink-0 rounded-full" />
          <div className="flex-1 space-y-1.5">
            <SkeletonBlock className={`h-3.5 ${i % 3 === 0 ? 'w-3/4' : i % 3 === 1 ? 'w-2/3' : 'w-1/2'}`} />
            <SkeletonBlock className="h-2.5 w-32" />
          </div>
          <SkeletonBlock className="h-3 w-16 flex-shrink-0" />
        </div>
      ))}
    </div>
  );
}

/** Skeleton for a diff viewer */
function DiffSkeleton() {
  return (
    <div className="divide-y divide-border">
      {Array.from({ length: 3 }).map((_, i) => (
        <div key={i}>
          {/* File header */}
          <div className="flex items-center gap-2 bg-navy-800/50 px-4 py-2">
            <SkeletonBlock className="h-4 w-4 rounded" />
            <SkeletonBlock className="h-4 w-5" />
            <SkeletonBlock className={`h-4 ${i === 0 ? 'w-48' : i === 1 ? 'w-64' : 'w-40'}`} />
          </div>
          {/* Hunk lines */}
          <div className="bg-diff-hunk-bg px-4 py-1">
            <SkeletonBlock className="h-3 w-48" />
          </div>
          {Array.from({ length: 6 }).map((_, j) => (
            <div key={j} className="flex items-center gap-0 border-b border-border/10 px-2 py-1">
              <SkeletonBlock className="mr-2 h-3 w-8 flex-shrink-0" />
              <SkeletonBlock className="mr-2 h-3 w-8 flex-shrink-0" />
              <SkeletonBlock className={`h-3 ${j % 4 === 1 ? 'bg-diff-add-bg/30 w-3/4' : j % 4 === 3 ? 'bg-diff-del-bg/30 w-2/3' : 'w-1/2'}`} />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

/** Generic table row skeleton */
function TableRowsSkeleton({ rows = 8, cols = 4 }: { rows?: number; cols?: number }) {
  return (
    <div className="divide-y divide-border/30">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="flex items-center gap-4 px-4 py-3">
          {Array.from({ length: cols }).map((_, j) => (
            <SkeletonBlock
              key={j}
              className={`h-4 ${j === 0 ? 'w-6' : j === cols - 1 ? 'w-16 flex-shrink-0' : 'flex-1'}`}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

/** Generic error state with retry button */
interface ErrorStateProps {
  message: string;
  onRetry?: () => void;
  children?: ReactNode;
}

function ErrorState({ message, onRetry, children }: ErrorStateProps) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-status-deleted/10">
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            className="text-status-deleted"
            aria-hidden="true"
          >
            <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
        </div>
        <p className="mb-1 text-sm font-semibold text-text-primary">Something went wrong</p>
        <p className="mb-4 text-xs text-text-secondary">{message}</p>
        {onRetry && (
          <button
            onClick={onRetry}
            className="rounded bg-accent px-4 py-2 text-sm font-medium text-navy-950 transition-colors hover:bg-accent-light"
          >
            Try Again
          </button>
        )}
        {children}
      </div>
    </div>
  );
}

/** Empty state with icon, message, and optional action */
interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
}

function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      {icon && (
        <div className="text-text-muted/40">{icon}</div>
      )}
      <p className="text-sm font-semibold text-text-secondary">{title}</p>
      {description && (
        <p className="max-w-xs text-xs text-text-muted">{description}</p>
      )}
      {action}
    </div>
  );
}

export {
  SkeletonBlock,
  RepoListSkeleton,
  CommitListSkeleton,
  DiffSkeleton,
  TableRowsSkeleton,
  ErrorState,
  EmptyState,
};
