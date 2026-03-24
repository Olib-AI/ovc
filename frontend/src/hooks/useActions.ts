import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as api from '../api/client.ts';
import type { ActionConfigDetail, RunActionsRequest } from '../api/types.ts';

export function useActionsList(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions'],
    queryFn: () => api.listActions(repoId!),
    enabled: !!repoId,
  });
}

export function useActionsConfig(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-config'],
    queryFn: () => api.getActionsConfig(repoId!),
    enabled: !!repoId,
  });
}

export function usePutActionsConfig(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (content: string) => api.putActionsConfig(repoId, content),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-config'] });
    },
  });
}

export function useActionsHistory(repoId: string | undefined, limit?: number) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-history', limit],
    queryFn: () => api.getActionsHistory(repoId!, limit),
    enabled: !!repoId,
  });
}

export function useActionRun(repoId: string | undefined, runId: string | null) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-run', runId],
    queryFn: () => api.getActionRun(repoId!, runId!),
    enabled: !!repoId && !!runId,
    gcTime: 30_000, // per-run data accumulates — GC after 30s unmounted
  });
}

export function useRunActions(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (req: RunActionsRequest) => api.runActions(repoId, req),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-history'] });
    },
  });
}

export function useRunSingleAction(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, fix }: { name: string; fix?: boolean }) =>
      api.runSingleAction(repoId, name, fix),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-history'] });
    },
  });
}

export function useInitActions(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (force?: boolean) => api.initActions(repoId, force),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-config'] });
    },
  });
}

export function useDetectLanguages(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-detect'],
    queryFn: () => api.detectLanguages(repoId!),
    enabled: !!repoId,
  });
}

export function useActionSecrets(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-secrets'],
    queryFn: () => api.listActionSecrets(repoId!),
    enabled: !!repoId,
  });
}

export function usePutActionSecret(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, value }: { name: string; value: string }) =>
      api.putActionSecret(repoId, name, value),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-secrets'] });
    },
  });
}

export function useDeleteActionSecret(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteActionSecret(repoId, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-secrets'] });
    },
  });
}

export function useDockerStatus(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'actions-docker-status'],
    queryFn: () => api.getDockerStatus(repoId!),
    enabled: !!repoId,
  });
}

// Per-action config CRUD

export function useActionConfig(repoId: string | undefined, name: string | null) {
  return useQuery({
    queryKey: ['repo', repoId, 'action-config', name],
    queryFn: () => api.getActionConfig(repoId!, name!),
    enabled: !!repoId && !!name,
  });
}

export function usePutActionConfig(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, config }: { name: string; config: ActionConfigDetail }) =>
      api.putActionConfig(repoId, name, config),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-config'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'action-config'] });
    },
  });
}

export function useDeleteActionConfig(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteActionConfig(repoId, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'actions-config'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'action-config'] });
    },
  });
}
