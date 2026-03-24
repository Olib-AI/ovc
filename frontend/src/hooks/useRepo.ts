import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as api from '../api/client.ts';
import type {
  CreateCommentPayload,
  CreatePullRequestPayload,
  CreateReviewPayload,
  GrantAccessPayload,
  PrMergeStrategy,
  PullRequestState,
  ResetMode,
  SetBranchProtectionPayload,
  UpdatePullRequestPayload,
} from '../api/types.ts';

export function useRepos() {
  return useQuery({
    queryKey: ['repos'],
    queryFn: api.listRepos,
  });
}

export function useRepo(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id],
    queryFn: () => api.getRepo(id!),
    enabled: !!id,
  });
}

export function useRepoStatus(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'status'],
    queryFn: ({ signal }) => api.getStatus(id!, signal),
    enabled: !!id,
    staleTime: 10_000, // keep fresh between interval refetches
    refetchInterval: 10_000,
    refetchIntervalInBackground: false,
    gcTime: 30_000, // aggressively GC status data when unmounted
  });
}

export function useFileTree(id: string | undefined, path?: string, ref?: string) {
  return useQuery({
    queryKey: ['repo', id, 'tree', path ?? '', ref ?? ''],
    queryFn: () => api.getTree(id!, path, ref),
    enabled: !!id,
  });
}

export function useFileContent(id: string | undefined, path: string | null, ref?: string) {
  return useQuery({
    queryKey: ['repo', id, 'blob', path, ref ?? ''],
    queryFn: () => api.getBlob(id!, path!, ref),
    enabled: !!id && !!path,
    gcTime: 30_000, // blobs can be large — GC after 30s unmounted
  });
}

export function useCommitLog(id: string | undefined, limit = 50, after?: string) {
  return useQuery({
    queryKey: ['repo', id, 'log', limit, after ?? ''],
    queryFn: () => api.getLog(id!, limit, after),
    enabled: !!id,
  });
}

export function useDiff(id: string | undefined, staged?: boolean) {
  return useQuery({
    queryKey: ['repo', id, 'diff', staged],
    queryFn: () => api.getDiff(id!, staged),
    enabled: !!id,
  });
}

export function useCommitDiff(id: string | undefined, commitId: string | null) {
  return useQuery({
    queryKey: ['repo', id, 'commitDiff', commitId],
    queryFn: () => api.getCommitDiff(id!, commitId!),
    enabled: !!id && !!commitId,
    gcTime: 10_000, // diffs are heavy — evict quickly after unmount
  });
}

export function useCommitFileDiff(id: string | undefined, commitId: string | null, filePath: string | null) {
  return useQuery({
    queryKey: ['repo', id, 'commitFileDiff', commitId, filePath],
    queryFn: () => api.getCommitFileDiff(id!, commitId!, filePath!),
    enabled: !!id && !!commitId && !!filePath,
    gcTime: 30_000,
  });
}

export function useBranches(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'branches'],
    queryFn: () => api.listBranches(id!),
    enabled: !!id,
  });
}

export function useTags(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'tags'],
    queryFn: () => api.listTags(id!),
    enabled: !!id,
  });
}

export function useStash(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'stash'],
    queryFn: () => api.listStash(id!),
    enabled: !!id,
  });
}

export function useSyncStatus(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'sync'],
    queryFn: () => api.getSyncStatus(id!),
    enabled: !!id,
  });
}

export function useCreateRepo() {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, password }: { name: string; password: string }) =>
      api.createRepo(name, password),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repos'] });
    },
  });
}

export function useDeleteRepo() {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (id: string) => api.deleteRepo(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repos'] });
    },
  });
}

export function useUnlockRepo() {
  return useMutation({
    gcTime: 0,
    mutationFn: ({ id, password }: { id: string; password: string }) =>
      api.unlockRepo(id, password),
  });
}

export function useStageFiles(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (paths: string[]) => api.stageFiles(id, paths),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useUnstageFiles(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (paths: string[]) => api.unstageFiles(id, paths),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useRestoreFiles(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (paths: string[]) => api.restoreFiles(id, paths),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useCleanFiles(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ paths, dryRun }: { paths?: string[]; dryRun?: boolean }) =>
      api.cleanFiles(id, paths, dryRun),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function useStagedDiff(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'diff', true],
    queryFn: () => api.getDiff(id!, true),
    enabled: !!id,
    staleTime: 30_000, // keep fresh between interval refetches to avoid redundant requests
    refetchInterval: 30_000,
    refetchIntervalInBackground: false,
    gcTime: 60_000, // diff data can be large — GC aggressively when unmounted
  });
}

export function useUnstagedDiff(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'diff', false],
    queryFn: () => api.getDiff(id!, false),
    enabled: !!id,
    staleTime: 30_000, // keep fresh between interval refetches to avoid redundant requests
    refetchInterval: 30_000,
    refetchIntervalInBackground: false,
    gcTime: 60_000, // diff data can be large — GC aggressively when unmounted
  });
}

export function useCreateCommit(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (params: {
      message: string;
      authorName: string;
      authorEmail: string;
      amend?: boolean;
      sign?: boolean;
    }) =>
      api.createCommit(id, params.message, params.authorName, params.authorEmail, {
        amend: params.amend,
        sign: params.sign,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
    },
  });
}

export function useCreateBranch(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, startPoint }: { name: string; startPoint?: string }) =>
      api.createBranch(id, name, startPoint),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id] });
    },
  });
}

export function useDeleteBranch(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteBranch(id, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
    },
  });
}

export function useCheckoutBranch(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.checkoutBranch(id, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id] });
    },
  });
}

export function useMergeBranch(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ target, source, strategy }: { target: string; source: string; strategy?: api.MergeStrategy }) =>
      api.mergeBranch(id, target, source, strategy),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'blob'] });
    },
  });
}

export function useCreateTag(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, commitId, message }: { name: string; commitId?: string; message?: string }) =>
      api.createTag(id, name, commitId, message),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tags'] });
    },
  });
}

export function useDeleteTag(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteTag(id, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tags'] });
    },
  });
}

export function usePushStash(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (message?: string) => api.pushStash(id, message),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'stash'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function usePopStash(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (idx: number) => api.popStash(id, idx),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'stash'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function useApplyStash(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (idx: number) => api.applyStash(id, idx),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'stash'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useDropStash(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (idx: number) => api.dropStash(id, idx),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'stash'] });
    },
  });
}

export function usePushSync(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: () => api.pushSync(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'sync'] });
    },
  });
}

export function usePullSync(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: () => api.pullSync(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tags'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function useGc(id: string) {
  return useMutation({
    gcTime: 0,
    mutationFn: () => api.gc(id),
  });
}

export function useResetCommit(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ commitId, mode }: { commitId?: string; mode: ResetMode }) =>
      api.resetCommit(id, commitId, mode),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'reflog'] });
    },
  });
}

export function useRebase(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (onto: string) => api.rebase(id, onto),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useCherryPick(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (commitId: string) => api.cherryPick(id, commitId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useRevertCommit(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (commitId: string) => api.revertCommit(id, commitId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

// Remotes
export function useRemotes(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'remotes'],
    queryFn: () => api.listRemotes(id!),
    enabled: !!id,
  });
}

export function useAddRemote(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, url, backendType }: { name: string; url: string; backendType: string }) =>
      api.addRemote(id, name, url, backendType),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'remotes'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'sync'] });
    },
  });
}

export function useDeleteRemote(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteRemote(id, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'remotes'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'sync'] });
    },
  });
}

// Notes
export function useNotes(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'notes'],
    queryFn: () => api.getNotes(id!),
    enabled: !!id,
  });
}

export function useNote(id: string | undefined, commitId: string | null) {
  return useQuery({
    queryKey: ['repo', id, 'notes', commitId],
    queryFn: () => api.getNote(id!, commitId!),
    enabled: !!id && !!commitId,
    retry: false,
    gcTime: 30_000, // per-commit notes accumulate — GC after 30s unmounted
  });
}

export function useSetNote(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ commitId, message }: { commitId: string; message: string }) =>
      api.setNote(id, commitId, message),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'notes'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'notes', variables.commitId] });
    },
  });
}

export function useDeleteNote(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (commitId: string) => api.deleteNote(id, commitId),
    onSuccess: (_data, commitId) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'notes'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'notes', commitId] });
    },
  });
}

// Describe
export function useDescribeCommit(id: string | undefined, commitId: string | null) {
  return useQuery({
    queryKey: ['repo', id, 'describe', commitId],
    queryFn: () => api.describeCommit(id!, commitId!),
    enabled: !!id && !!commitId,
    retry: false,
    gcTime: 30_000, // per-commit describe accumulates — GC after 30s unmounted
  });
}

// Submodules
export function useSubmodules(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'submodules'],
    queryFn: () => api.getSubmodules(id!),
    enabled: !!id,
  });
}

export function useAddSubmodule(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ name, path, url }: { name: string; path: string; url: string }) =>
      api.addSubmodule(id, name, path, url),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'submodules'] });
    },
  });
}

export function useDeleteSubmodule(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (name: string) => api.deleteSubmodule(id, name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'submodules'] });
    },
  });
}

// Shortlog
export function useShortlog(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'shortlog'],
    queryFn: () => api.getShortlog(id!),
    enabled: !!id,
  });
}

// Clear all stashes
export function useClearStash(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: () => api.clearStash(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'stash'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

// File CRUD
export function usePutBlob(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ path, content, encoding }: { path: string; content: string; encoding?: 'utf8' | 'base64' }) =>
      api.putBlob(id, path, content, encoding),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'blob'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useDeleteBlob(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (path: string) => api.deleteBlob(id, path),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'blob'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useUploadFiles(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ targetPath, files }: { targetPath: string; files: File[] }) =>
      api.uploadFiles(id, targetPath, files),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

export function useCreateDirectory(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (path: string) => api.createDirectory(id, path),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function useMoveFile(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ fromPath, toPath }: { fromPath: string; toPath: string }) =>
      api.moveFile(id, fromPath, toPath),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'blob'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'diff'] });
    },
  });
}

// File history — commits that touched a specific path
export function useFileHistory(
  repoId: string | undefined,
  path: string | null,
  limit = 50,
) {
  return useQuery({
    queryKey: ['repo', repoId, 'fileHistory', path, limit],
    queryFn: () => api.getFileHistory(repoId!, path!, limit),
    enabled: !!repoId && !!path,
  });
}

// Pull Request / Branch Comparison
export function usePullRequestView(id: string | undefined, branch: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'pulls', branch],
    queryFn: () => api.getPullRequestView(id!, branch!),
    enabled: !!id && !!branch,
    gcTime: 30_000, // PR view diffs are heavy — GC after 30s unmounted
  });
}

// PR Lifecycle
export function useListPullRequests(
  id: string | undefined,
  state?: PullRequestState | 'all',
) {
  return useQuery({
    queryKey: ['repo', id, 'pullRequests', state ?? 'all'],
    queryFn: () => api.listPullRequests(id!, state),
    enabled: !!id,
  });
}

export function useCreatePullRequest(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (payload: CreatePullRequestPayload) => api.createPullRequest(id, payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequests'] });
    },
  });
}

export function useGetPullRequest(id: string | undefined, number: number | null) {
  return useQuery({
    queryKey: ['repo', id, 'pullRequest', number],
    queryFn: () => api.getPullRequestByNumber(id!, number!),
    enabled: !!id && number !== null,
  });
}

export function useUpdatePullRequest(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ number, payload }: { number: number; payload: UpdatePullRequestPayload }) =>
      api.updatePullRequest(id, number, payload),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequests'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.number] });
    },
  });
}

export function useMergePullRequest(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ number, strategy, force }: { number: number; strategy?: PrMergeStrategy; force?: boolean }) =>
      api.mergePullRequest(id, number, strategy, force),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequests'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.number] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'tree'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'blob'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'status'] });
    },
  });
}

export function useRunPrChecks(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (prNumber: number) => api.runPrChecks(id, prNumber),
    onSuccess: (_data, prNumber) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', prNumber] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequests'] });
    },
  });
}

export function useCreatePrFromProposal(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (branch: string) => api.createPrFromProposal(id, branch),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'dependencyProposals'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequests'] });
    },
  });
}

// Compare two refs
export function useCompare(
  repoId: string | undefined,
  base: string,
  head: string,
) {
  return useQuery({
    queryKey: ['repo', repoId, 'compare', base, head],
    queryFn: () => api.compareCommits(repoId!, base, head),
    enabled: !!repoId && !!base && !!head && base !== head,
    gcTime: 30_000, // compare diffs are heavy — GC after 30s unmounted
  });
}

// Dependencies — not auto-fetched: uses enabled:false + manual refetch
// because the backend makes outbound network calls to package registries.
export function useDependencies(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'dependencies'],
    queryFn: () => api.getDependencies(repoId!),
    enabled: false,
    retry: false,
    gcTime: 5 * 60_000, // cache result for 5 minutes
  });
}

// Dependency proposals — lists existing deps/* branches with merge status.
// Auto-fetches when repoId is present (lightweight query, no outbound network calls).
export function useDependencyProposals(repoId: string | undefined) {
  return useQuery({
    queryKey: ['repo', repoId, 'dependencyProposals'],
    queryFn: () => api.listDependencyProposals(repoId!),
    enabled: !!repoId,
    retry: false,
  });
}

export function useCreateDependencyUpdates(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (updates?: Parameters<typeof api.createDependencyUpdates>[1]) =>
      api.createDependencyUpdates(repoId, updates),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'dependencyProposals'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'branches'] });
    },
  });
}

export function useDeleteDependencyProposal(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (branch: string) => api.deleteDependencyProposal(repoId, branch),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'dependencyProposals'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'branches'] });
    },
  });
}

export function useMergeDependencyProposal(repoId: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (branch: string) => api.mergeDependencyProposal(repoId, branch),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'dependencyProposals'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'branches'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'log'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId, 'status'] });
      void qc.invalidateQueries({ queryKey: ['repo', repoId] });
    },
  });
}

// Access Control
export function useListAccess(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'access'],
    queryFn: () => api.listAccess(id!),
    enabled: !!id,
  });
}

export function useGrantAccess(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (payload: GrantAccessPayload) => api.grantAccess(id, payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'access'] });
    },
  });
}

export function useRevokeAccess(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (fingerprint: string) => api.revokeAccess(id, fingerprint),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'access'] });
    },
  });
}

export function useSetRole(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ fingerprint, role }: { fingerprint: string; role: string }) =>
      api.setRole(id, fingerprint, { role }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'access'] });
    },
  });
}

export function useListBranchProtection(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'branchProtection'],
    queryFn: () => api.listBranchProtection(id!),
    enabled: !!id,
  });
}

export function useSetBranchProtection(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ branch, payload }: { branch: string; payload: SetBranchProtectionPayload }) =>
      api.setBranchProtection(id, branch, payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branchProtection'] });
    },
  });
}

export function useRemoveBranchProtection(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (branch: string) => api.removeBranchProtection(id, branch),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'branchProtection'] });
    },
  });
}

// PR Reviews & Comments
export function useListReviews(id: string | undefined, prNumber: number | null) {
  return useQuery({
    queryKey: ['repo', id, 'pullRequest', prNumber, 'reviews'],
    queryFn: () => api.listReviews(id!, prNumber!),
    enabled: !!id && prNumber !== null,
  });
}

export function useCreateReview(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ prNumber, payload }: { prNumber: number; payload: CreateReviewPayload }) =>
      api.createReview(id, prNumber, payload),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.prNumber, 'reviews'] });
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.prNumber] });
    },
  });
}

export function useListComments(id: string | undefined, prNumber: number | null) {
  return useQuery({
    queryKey: ['repo', id, 'pullRequest', prNumber, 'comments'],
    queryFn: () => api.listComments(id!, prNumber!),
    enabled: !!id && prNumber !== null,
  });
}

export function useCreateComment(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ prNumber, payload }: { prNumber: number; payload: CreateCommentPayload }) =>
      api.createComment(id, prNumber, payload),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.prNumber, 'comments'] });
    },
  });
}

export function useUpdateComment(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ prNumber, commentId, body }: { prNumber: number; commentId: number; body: string }) =>
      api.updateComment(id, prNumber, commentId, body),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.prNumber, 'comments'] });
    },
  });
}

export function useDeleteComment(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: ({ prNumber, commentId }: { prNumber: number; commentId: number }) =>
      api.deleteComment(id, prNumber, commentId),
    onSuccess: (_data, variables) => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'pullRequest', variables.prNumber, 'comments'] });
    },
  });
}

// Repo Config
export function useRepoConfig(id: string | undefined) {
  return useQuery({
    queryKey: ['repo', id, 'config'],
    queryFn: () => api.getRepoConfig(id!),
    enabled: !!id,
  });
}

export function useUpdateRepoConfig(id: string) {
  const qc = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (payload: Parameters<typeof api.updateRepoConfig>[1]) =>
      api.updateRepoConfig(id, payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['repo', id, 'config'] });
    },
  });
}
