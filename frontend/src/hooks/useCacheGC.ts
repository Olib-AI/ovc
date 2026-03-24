import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';

const GC_INTERVAL_MS = 60_000;
const INACTIVE_MAX_AGE_MS = 30_000;
const MAX_CACHE_ENTRIES = 50;

/**
 * Periodic garbage collector for the React Query cache.
 *
 * React Query's built-in `gcTime` only starts its timer once the last
 * observer unmounts.  When a user stays on a single page for hours
 * (e.g. HistoryPage clicking through commits), every fetched query
 * accumulates because the page — and therefore the observer — never
 * unmounts.
 *
 * This hook runs a sweep every 60 seconds that:
 * 1. Removes inactive queries (0 observers) older than 30 s.
 * 2. If the cache still exceeds 50 entries, evicts the oldest inactive
 *    queries first (heavy payloads like diffs, blobs, blame, search).
 *
 * It never touches queries that have active observers.
 */
export function useCacheGC() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const interval = setInterval(() => {
      const cache = queryClient.getQueryCache();
      const now = Date.now();
      const queries = cache.getAll();

      // Phase 1 — remove stale inactive queries
      for (const query of queries) {
        if (
          query.getObserversCount() === 0 &&
          now - query.state.dataUpdatedAt > INACTIVE_MAX_AGE_MS
        ) {
          cache.remove(query);
        }
      }

      // Phase 2 — enforce hard cap on total entries
      const remaining = cache.getAll();
      if (remaining.length > MAX_CACHE_ENTRIES) {
        const inactive = remaining
          .filter((q) => q.getObserversCount() === 0)
          .sort((a, b) => a.state.dataUpdatedAt - b.state.dataUpdatedAt);

        const excess = remaining.length - MAX_CACHE_ENTRIES;
        const toRemove = inactive.slice(0, excess);
        for (const q of toRemove) {
          cache.remove(q);
        }
      }
    }, GC_INTERVAL_MS);

    return () => clearInterval(interval);
  }, [queryClient]);
}
