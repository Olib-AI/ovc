import { useCallback, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as api from '../api/client.ts';
import type { UpdateLlmConfigPayload } from '../api/types.ts';

/** Progress info emitted during multi-pass LLM processing. */
export interface LlmProgress {
  phase: 'analyzing' | 'generating';
  batch?: number;
  total?: number;
  files?: string[];
}

/** Result type for the streaming LLM hook. */
export interface UseLlmStreamResult {
  /** Accumulated content from the LLM stream. */
  content: string;
  /** Whether the stream is currently active. */
  isStreaming: boolean;
  /** Error message, if any. */
  error: string | null;
  /** Multi-pass progress (null for single-pass). */
  progress: LlmProgress | null;
  /** Start streaming. */
  start: () => void;
  /** Cancel the active stream. */
  cancel: () => void;
  /** Reset content and error state. */
  reset: () => void;
}

/**
 * Maximum accumulated content size (in characters) before the stream is
 * automatically aborted.  Prevents runaway accumulation when the backend
 * or LLM server misbehaves (e.g. repeating error messages).
 */
const MAX_CONTENT_CHARS = 50_000;

/**
 * Hook for consuming a streaming LLM response.
 *
 * @param streamFn - Function that returns an async generator of text chunks.
 *                   Receives an AbortSignal for cancellation.
 */
export function useLlmStream(
  streamFn: (signal: AbortSignal) => AsyncGenerator<string, void, unknown>,
): UseLlmStreamResult {
  const [content, setContent] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<LlmProgress | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const start = useCallback(() => {
    setContent('');
    setError(null);
    setProgress(null);
    setIsStreaming(true);
    const controller = new AbortController();
    abortRef.current = controller;

    (async () => {
      let totalLen = 0;
      try {
        for await (const chunk of streamFn(controller.signal)) {
          // Progress events are prefixed with \x00progress:
          if (chunk.startsWith('\x00progress:')) {
            try {
              const data = JSON.parse(chunk.slice(10)) as LlmProgress;
              setProgress(data);
            } catch { /* ignore malformed progress */ }
            continue;
          }

          totalLen += chunk.length;
          if (totalLen > MAX_CONTENT_CHARS) {
            controller.abort();
            setError('Response too large — generation stopped');
            break;
          }
          setContent((prev) => prev + chunk);
        }
      } catch (e) {
        if (e instanceof DOMException && e.name === 'AbortError') {
          // Cancelled by user or safety limit — not an error unless already set.
        } else {
          setError(e instanceof Error ? e.message : 'Unknown error');
        }
      } finally {
        setIsStreaming(false);
        setProgress(null);
      }
    })();
  }, [streamFn]);

  const cancel = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  const reset = useCallback(() => {
    setContent('');
    setError(null);
    setProgress(null);
  }, []);

  return { content, isStreaming, error, progress, start, cancel, reset };
}

/** Fetches the per-repo LLM configuration. */
export function useLlmConfig(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'llm', 'config'],
    queryFn: () => api.getLlmConfig(repoId!),
    enabled: !!repoId,
    staleTime: 60_000,
    retry: false,
  });
}

/** Mutation to update the per-repo LLM configuration. */
export function useUpdateLlmConfig(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (payload: UpdateLlmConfigPayload) => api.updateLlmConfig(repoId, payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'llm', 'config'] });
    },
  });
}

/** Checks whether the LLM is configured and reachable (server + optional repo config). */
export function useLlmHealth(repoId?: string) {
  return useQuery({
    queryKey: ['llm', 'health', repoId],
    queryFn: () => api.getLlmHealth(repoId),
    staleTime: 30_000,
    retry: false,
  });
}
