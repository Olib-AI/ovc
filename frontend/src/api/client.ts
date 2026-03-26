import axios from 'axios';
import type {
  ActionConfigDetail,
  ActionConfigResponse,
  ActionHistoryListResponse,
  ActionListResponse,
  ActionsConfigResponse,
  BlameResponse,
  BranchInfo,
  BranchProtectionInfo,
  ChallengeResponse,
  CherryPickResponse,
  CleanResponse,
  CommitInfo,
  CommitLog,
  CompareResponse,
  CreateCommentPayload,
  CreatePullRequestPayload,
  CreateReviewPayload,
  DepMergeResponse,
  DependencyProposal,
  DependencyProposalsResponse,
  DependencyReport,
  DependencyUpdateResponse,
  DescribeResponse,
  DetectionResponse,
  DiffResponse,
  FileDiff,
  DocIndexResponse,
  DockerStatusResponse,
  DocSearchResponse,
  DocSectionResponse,
  FileContent,
  FileHistoryResponse,
  FileTreeEntry,
  GcResponse,
  GrantAccessPayload,
  KeyAuthPayload,
  ListAccessResponse,
  MergeResponse,
  NoteResponse,
  PrChecks,
  PrComment,
  PrMergeStrategy,
  PrReview,
  PullRequestFull,
  PullRequestState,
  PullRequestSummary,
  PullRequestView,
  PullResponse,
  PushResponse,
  RebaseResponse,
  ReflogEntry,
  RemoteInfo,
  RepoConfigResponse,
  RepoInfo,
  ResetMode,
  ResetResponse,
  RevertResponse,
  RunActionsRequest,
  RunActionsResponse,
  SearchResponse,
  SecretDeleteResponse,
  SecretPutResponse,
  SecretsListResponse,
  SetBranchProtectionPayload,
  SetRolePayload,
  ShortlogResponse,
  StashEntryInfo,
  StatusResponse,
  SubmoduleInfo,
  SyncStatusResponse,
  TagInfo,
  TokenResponse,
  UpdatePullRequestPayload,
  UpdateRepoConfigPayload,
  UserAccessInfo,
  LlmConfigResponse,
  LlmHealthResponse,
  LlmDescriptionResponse,
  UpdateLlmConfigPayload,
} from './types.ts';

const api = axios.create({
  baseURL: '/api/v1',
  headers: { 'Content-Type': 'application/json' },
});

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('ovc_token');
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

api.interceptors.response.use(
  (response) => response,
  (error: unknown) => {
    if (axios.isAxiosError(error)) {
      if (error.response?.status === 401) {
        localStorage.removeItem('ovc_token');
        // Use a debounced redirect to avoid race conditions with multiple
        // concurrent queries. Only redirect once per 2-second window.
        if (!window.location.pathname.startsWith('/login')) {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const w = window as any;
          if (!w.__ovc_redirecting) {
            w.__ovc_redirecting = true;
            setTimeout(() => { w.__ovc_redirecting = false; }, 2000);
            window.location.href = '/login';
          }
        }
      }

      // Network error (no response received — server unreachable, DNS failure, etc.)
      if (!error.response && error.request) {
        error.message = 'Connection lost. Check your network and try again.';
        return Promise.reject(error);
      }

      // Friendly messages for well-known HTTP status codes
      if (error.response) {
        const data = error.response.data as
          | { error?: { message?: string }; message?: string }
          | undefined;

        if (data?.error?.message) {
          error.message = data.error.message;
        } else if (data?.message) {
          error.message = data.message;
        } else {
          switch (error.response.status) {
            case 403:
              error.message = 'You do not have permission to perform this action.';
              break;
            case 404:
              error.message = 'The requested resource was not found.';
              break;
            case 409:
              error.message = 'Conflict: the resource was modified by another operation.';
              break;
            case 422:
              error.message = 'Invalid request data. Check your inputs and try again.';
              break;
            case 429:
              error.message = 'Too many requests. Please wait a moment and try again.';
              break;
            case 500:
              error.message = 'Server error. Please try again later.';
              break;
            case 502:
            case 503:
            case 504:
              error.message = 'Service temporarily unavailable. Please try again later.';
              break;
            default:
              // Keep the default axios message for uncommon codes
              break;
          }
        }
      }
    }
    return Promise.reject(error);
  },
);

// Auth
export async function getToken(password: string): Promise<TokenResponse> {
  const { data } = await api.post<TokenResponse>('/auth/token', { password });
  return data;
}

export async function verifyToken(): Promise<boolean> {
  try {
    await api.get('/auth/verify');
    return true;
  } catch {
    return false;
  }
}

// Repos
export async function listRepos(): Promise<RepoInfo[]> {
  const { data } = await api.get<RepoInfo[]>('/repos');
  return data;
}

export async function createRepo(name: string, password: string): Promise<RepoInfo> {
  const { data } = await api.post<RepoInfo>('/repos', { name, password });
  return data;
}

export async function getRepo(id: string): Promise<RepoInfo> {
  const { data } = await api.get<RepoInfo>(`/repos/${id}`);
  return data;
}

export async function deleteRepo(id: string): Promise<void> {
  await api.delete(`/repos/${id}`);
}

export async function unlockRepo(id: string, password: string): Promise<void> {
  await api.post(`/repos/${id}/unlock`, { password });
}

// Repo Config
export async function getRepoConfig(repoId: string): Promise<RepoConfigResponse> {
  const { data } = await api.get<RepoConfigResponse>(`/repos/${repoId}/config`);
  return data;
}

export async function updateRepoConfig(
  repoId: string,
  payload: UpdateRepoConfigPayload,
): Promise<RepoConfigResponse> {
  const { data } = await api.put<RepoConfigResponse>(`/repos/${repoId}/config`, payload);
  return data;
}

// Files
export async function getStatus(id: string, signal?: AbortSignal): Promise<StatusResponse> {
  const { data } = await api.get<StatusResponse>(`/repos/${id}/status`, { signal });
  return data;
}

export async function getTree(id: string, path?: string, ref?: string): Promise<FileTreeEntry[]> {
  const params: Record<string, string> = {};
  if (path) params.path = path;
  if (ref) params.ref = ref;
  const { data } = await api.get<FileTreeEntry[]>(`/repos/${id}/tree`, {
    params: Object.keys(params).length > 0 ? params : undefined,
  });
  return data;
}

export async function getBlob(id: string, path: string, ref?: string): Promise<FileContent> {
  const params: Record<string, string> = { path };
  if (ref) params.ref = ref;
  const { data } = await api.get<FileContent>(`/repos/${id}/blob`, { params });
  return data;
}

export async function stageFiles(id: string, paths: string[]): Promise<void> {
  await api.post(`/repos/${id}/stage`, { paths });
}

export async function unstageFiles(id: string, paths: string[]): Promise<void> {
  await api.post(`/repos/${id}/unstage`, { paths });
}

export async function restoreFiles(id: string, paths: string[]): Promise<void> {
  await api.post(`/repos/${id}/restore`, { paths });
}

export async function cleanFiles(
  id: string,
  paths?: string[],
  dryRun?: boolean,
): Promise<CleanResponse> {
  const { data } = await api.post<CleanResponse>(`/repos/${id}/clean`, {
    ...(paths ? { paths } : {}),
    ...(dryRun ? { dry_run: true } : {}),
  });
  return data;
}

// Commits
export async function getLog(id: string, limit = 50, after?: string): Promise<CommitLog> {
  const params: Record<string, string | number> = { limit };
  if (after) params.after = after;
  const { data } = await api.get<CommitLog>(`/repos/${id}/log`, { params });
  return data;
}

export async function createCommit(
  id: string,
  message: string,
  authorName: string,
  authorEmail: string,
  options?: { amend?: boolean; sign?: boolean },
): Promise<CommitInfo> {
  const { data } = await api.post<CommitInfo>(`/repos/${id}/commits`, {
    message,
    author_name: authorName,
    author_email: authorEmail,
    ...(options?.amend ? { amend: true } : {}),
    ...(options?.sign ? { sign: true } : {}),
  });
  return data;
}

export async function getCommit(id: string, commitId: string): Promise<CommitInfo> {
  const { data } = await api.get<CommitInfo>(`/repos/${id}/commits/${commitId}`);
  return data;
}

export async function getDiff(id: string, staged?: boolean): Promise<DiffResponse> {
  const { data } = await api.get<DiffResponse>(`/repos/${id}/diff`, {
    params: staged !== undefined ? { staged } : undefined,
  });
  return data;
}

export async function getCommitDiff(id: string, commitId: string, statsOnly?: boolean): Promise<DiffResponse> {
  const { data } = await api.get<DiffResponse>(`/repos/${id}/diff/${commitId}`, {
    params: statsOnly ? { stats_only: true } : undefined,
  });
  return data;
}

export async function getCommitFileDiff(id: string, commitId: string, filePath: string): Promise<FileDiff> {
  const { data } = await api.get<FileDiff>(`/repos/${id}/diff/${commitId}/file`, {
    params: { path: filePath },
  });
  return data;
}

// Branches
export async function listBranches(id: string): Promise<BranchInfo[]> {
  const { data } = await api.get<BranchInfo[]>(`/repos/${id}/branches`);
  return data;
}

export async function createBranch(id: string, name: string, startPoint?: string): Promise<BranchInfo> {
  const { data } = await api.post<BranchInfo>(`/repos/${id}/branches`, {
    name,
    ...(startPoint ? { start_point: startPoint } : {}),
  });
  return data;
}

export async function deleteBranch(id: string, name: string): Promise<void> {
  await api.delete(`/repos/${id}/branches/${encodeURIComponent(name)}`);
}

export async function checkoutBranch(id: string, name: string): Promise<void> {
  await api.post(`/repos/${id}/branches/${encodeURIComponent(name)}/checkout`);
}

export type MergeStrategy = 'merge' | 'squash' | 'rebase';

export async function mergeBranch(
  id: string,
  name: string,
  sourceBranch: string,
  strategy?: MergeStrategy,
): Promise<MergeResponse> {
  const { data } = await api.post<MergeResponse>(`/repos/${id}/branches/${encodeURIComponent(name)}/merge`, {
    source_branch: sourceBranch,
    ...(strategy ? { strategy } : {}),
  });
  return data;
}

// Pull Request / Branch Comparison
export async function getPullRequestView(id: string, branch: string): Promise<PullRequestView> {
  const { data } = await api.get<PullRequestView>(`/repos/${id}/pulls/${encodeURIComponent(branch)}`);
  return data;
}

// PR Lifecycle
export async function listPullRequests(
  id: string,
  state?: PullRequestState | 'all',
): Promise<PullRequestSummary[]> {
  const { data } = await api.get<PullRequestSummary[]>(`/repos/${id}/pulls`, {
    params: state ? { state } : undefined,
  });
  return data;
}

export async function createPullRequest(
  id: string,
  payload: CreatePullRequestPayload,
): Promise<PullRequestFull> {
  const { data } = await api.post<PullRequestFull>(`/repos/${id}/pulls`, payload);
  return data;
}

export async function getPullRequestByNumber(
  id: string,
  number: number,
): Promise<PullRequestFull> {
  const { data } = await api.get<PullRequestFull>(`/repos/${id}/pulls/by-number/${number}`);
  return data;
}

export async function updatePullRequest(
  id: string,
  number: number,
  payload: UpdatePullRequestPayload,
): Promise<PullRequestFull> {
  const { data } = await api.patch<PullRequestFull>(
    `/repos/${id}/pulls/by-number/${number}`,
    payload,
  );
  return data;
}

export async function mergePullRequest(
  id: string,
  number: number,
  strategy?: PrMergeStrategy,
  force?: boolean,
): Promise<PullRequestFull> {
  const { data } = await api.post<PullRequestFull>(
    `/repos/${id}/pulls/by-number/${number}/merge`,
    {
      ...(strategy ? { strategy } : {}),
      ...(force ? { force: true } : {}),
    },
  );
  return data;
}

export async function runPrChecks(
  repoId: string,
  prNumber: number,
): Promise<PrChecks> {
  const { data } = await api.post<PrChecks>(
    `/repos/${repoId}/pulls/by-number/${prNumber}/checks`,
  );
  return data;
}

export async function createPrFromProposal(
  repoId: string,
  branch: string,
): Promise<PullRequestFull> {
  const { data } = await api.post<PullRequestFull>(
    `/repos/${repoId}/dependencies/proposals/${encodeURIComponent(branch)}/create-pr`,
  );
  return data;
}

// Tags
export async function listTags(id: string): Promise<TagInfo[]> {
  const { data } = await api.get<TagInfo[]>(`/repos/${id}/tags`);
  return data;
}

export async function createTag(id: string, name: string, commitId?: string, message?: string): Promise<TagInfo> {
  const { data } = await api.post<TagInfo>(`/repos/${id}/tags`, {
    name,
    ...(commitId ? { commit_id: commitId } : {}),
    ...(message ? { message } : {}),
  });
  return data;
}

export async function deleteTag(id: string, name: string): Promise<void> {
  await api.delete(`/repos/${id}/tags/${encodeURIComponent(name)}`);
}

// Stash
export async function listStash(id: string): Promise<StashEntryInfo[]> {
  const { data } = await api.get<StashEntryInfo[]>(`/repos/${id}/stash`);
  return data;
}

export async function pushStash(id: string, message?: string): Promise<StashEntryInfo> {
  const { data } = await api.post<StashEntryInfo>(`/repos/${id}/stash`, {
    message: message ?? 'WIP',
  });
  return data;
}

export async function popStash(id: string, idx: number): Promise<void> {
  await api.post(`/repos/${id}/stash/${idx}/pop`);
}

export async function applyStash(id: string, idx: number): Promise<void> {
  await api.post(`/repos/${id}/stash/${idx}/apply`);
}

export async function dropStash(id: string, idx: number): Promise<void> {
  await api.delete(`/repos/${id}/stash/${idx}`);
}

export async function clearStash(id: string): Promise<void> {
  await api.delete(`/repos/${id}/stash`);
}

// Sync
export async function getSyncStatus(id: string): Promise<SyncStatusResponse> {
  const { data } = await api.get<SyncStatusResponse>(`/repos/${id}/sync/status`);
  return data;
}

export async function pushSync(id: string): Promise<PushResponse> {
  const { data } = await api.post<PushResponse>(`/repos/${id}/sync/push`);
  return data;
}

export async function pullSync(id: string): Promise<PullResponse> {
  const { data } = await api.post<PullResponse>(`/repos/${id}/sync/pull`);
  return data;
}

// Remotes
export async function listRemotes(id: string): Promise<RemoteInfo[]> {
  const { data } = await api.get<RemoteInfo[]>(`/repos/${id}/remotes`);
  return data;
}

export async function addRemote(
  id: string,
  name: string,
  url: string,
  backendType: string,
): Promise<RemoteInfo> {
  const { data } = await api.post<RemoteInfo>(`/repos/${id}/remotes`, {
    name,
    url,
    backend_type: backendType,
  });
  return data;
}

export async function deleteRemote(id: string, name: string): Promise<void> {
  await api.delete(`/repos/${id}/remotes/${name}`);
}

// Advanced
export async function rebase(id: string, onto: string): Promise<RebaseResponse> {
  const { data } = await api.post<RebaseResponse>(`/repos/${id}/rebase`, { onto });
  return data;
}

export async function cherryPick(id: string, commitId: string): Promise<CherryPickResponse> {
  const { data } = await api.post<CherryPickResponse>(`/repos/${id}/cherry-pick`, {
    commit_id: commitId,
  });
  return data;
}

export async function revertCommit(id: string, commitId: string): Promise<RevertResponse> {
  const { data } = await api.post<RevertResponse>(`/repos/${id}/revert`, {
    commit_id: commitId,
  });
  return data;
}

export async function gc(id: string): Promise<GcResponse> {
  const { data } = await api.post<GcResponse>(`/repos/${id}/gc`);
  return data;
}

// Reset
export async function resetCommit(id: string, commitId: string | undefined, mode: ResetMode): Promise<ResetResponse> {
  const { data } = await api.post<ResetResponse>(`/repos/${id}/reset`, {
    ...(commitId ? { commit_id: commitId } : {}),
    mode,
  });
  return data;
}

// Actions
export async function getActionsConfig(id: string): Promise<ActionsConfigResponse> {
  const { data } = await api.get<ActionsConfigResponse>(`/repos/${id}/actions/config`);
  return data;
}

export async function putActionsConfig(id: string, content: string): Promise<void> {
  await api.put(`/repos/${id}/actions/config`, { content });
}

export async function listActions(id: string): Promise<ActionListResponse> {
  const { data } = await api.get<ActionListResponse>(`/repos/${id}/actions/list`);
  return data;
}

export async function runActions(id: string, req: RunActionsRequest): Promise<RunActionsResponse> {
  const { data } = await api.post<RunActionsResponse>(`/repos/${id}/actions/run`, req);
  return data;
}

export async function runSingleAction(
  id: string,
  name: string,
  fix?: boolean,
): Promise<RunActionsResponse> {
  const { data } = await api.post<RunActionsResponse>(`/repos/${id}/actions/run/${name}`, {
    fix: fix ?? false,
  });
  return data;
}

export async function detectLanguages(id: string): Promise<DetectionResponse> {
  const { data } = await api.get<DetectionResponse>(`/repos/${id}/actions/detect`);
  return data;
}

export async function getActionsHistory(
  id: string,
  limit?: number,
): Promise<ActionHistoryListResponse> {
  const { data } = await api.get<ActionHistoryListResponse>(`/repos/${id}/actions/history`, {
    params: limit !== undefined ? { limit } : undefined,
  });
  return data;
}

export async function clearActionsHistory(id: string): Promise<{ removed: number }> {
  const { data } = await api.delete<{ removed: number }>(`/repos/${id}/actions/history`);
  return data;
}

export async function getActionRun(id: string, runId: string): Promise<RunActionsResponse> {
  const { data } = await api.get<RunActionsResponse>(`/repos/${id}/actions/history/${runId}`);
  return data;
}

export async function initActions(id: string, force?: boolean): Promise<void> {
  await api.post(`/repos/${id}/actions/init`, { force: force ?? false });
}

// Action Secrets
export async function listActionSecrets(id: string): Promise<SecretsListResponse> {
  const { data } = await api.get<SecretsListResponse>(`/repos/${id}/actions/secrets`);
  return data;
}

export async function putActionSecret(
  id: string,
  name: string,
  value: string,
): Promise<SecretPutResponse> {
  const { data } = await api.put<SecretPutResponse>(
    `/repos/${id}/actions/secrets/${encodeURIComponent(name)}`,
    { value },
  );
  return data;
}

export async function deleteActionSecret(
  id: string,
  name: string,
): Promise<SecretDeleteResponse> {
  const { data } = await api.delete<SecretDeleteResponse>(
    `/repos/${id}/actions/secrets/${encodeURIComponent(name)}`,
  );
  return data;
}

// Docker Status
export async function getDockerStatus(id: string): Promise<DockerStatusResponse> {
  const { data } = await api.get<DockerStatusResponse>(`/repos/${id}/actions/docker/status`);
  return data;
}

// File CRUD
export async function putBlob(
  repoId: string,
  path: string,
  content: string,
  encoding: 'utf8' | 'base64' = 'utf8',
): Promise<void> {
  await api.put(`/repos/${repoId}/blob`, { path, content, encoding });
}

export async function deleteBlob(repoId: string, path: string): Promise<void> {
  await api.delete(`/repos/${repoId}/blob`, { data: { path } });
}

export async function uploadFiles(
  repoId: string,
  targetPath: string,
  files: File[],
): Promise<void> {
  const formData = new FormData();
  formData.append('path', targetPath);
  for (const file of files) {
    formData.append('file', file);
  }
  await api.post(`/repos/${repoId}/upload`, formData, {
    headers: { 'Content-Type': 'multipart/form-data' },
  });
}

export async function createDirectory(repoId: string, path: string): Promise<void> {
  await api.post(`/repos/${repoId}/mkdir`, { path });
}

export async function moveFile(repoId: string, fromPath: string, toPath: string): Promise<void> {
  await api.post(`/repos/${repoId}/move`, { from_path: fromPath, to_path: toPath });
}

// Blame
export async function getBlame(repoId: string, path: string, ref?: string): Promise<BlameResponse> {
  const { data } = await api.get<BlameResponse>(`/repos/${repoId}/blame/${path}`, {
    params: ref ? { ref } : undefined,
  });
  return data;
}

// Search
export async function searchCode(
  repoId: string,
  query: string,
  caseInsensitive = false,
  filePattern?: string,
  isRegex = false,
): Promise<SearchResponse> {
  const params: Record<string, string | boolean> = { q: query, case_insensitive: caseInsensitive };
  if (filePattern) params.file_pattern = filePattern;
  if (isRegex) params.is_regex = true;
  const { data } = await api.get<SearchResponse>(`/repos/${repoId}/search`, { params });
  return data;
}

// Notes
export async function getNotes(repoId: string): Promise<NoteResponse[]> {
  const { data } = await api.get<NoteResponse[]>(`/repos/${repoId}/notes`);
  return data;
}

export async function getNote(repoId: string, commitId: string): Promise<NoteResponse> {
  const { data } = await api.get<NoteResponse>(`/repos/${repoId}/notes/${commitId}`);
  return data;
}

export async function setNote(repoId: string, commitId: string, message: string): Promise<void> {
  await api.put(`/repos/${repoId}/notes/${commitId}`, { message });
}

export async function deleteNote(repoId: string, commitId: string): Promise<void> {
  await api.delete(`/repos/${repoId}/notes/${commitId}`);
}

// Reflog
export async function getReflog(repoId: string, limit = 50): Promise<ReflogEntry[]> {
  const { data } = await api.get<ReflogEntry[]>(`/repos/${repoId}/reflog`, { params: { limit } });
  return data;
}

// Describe
export async function describeCommit(repoId: string, commitId: string): Promise<DescribeResponse> {
  const { data } = await api.get<DescribeResponse>(`/repos/${repoId}/describe/${commitId}`);
  return data;
}

// Submodules
export async function getSubmodules(repoId: string): Promise<SubmoduleInfo[]> {
  const { data } = await api.get<SubmoduleInfo[]>(`/repos/${repoId}/submodules`);
  return data;
}

export async function addSubmodule(
  repoId: string,
  name: string,
  path: string,
  url: string,
): Promise<SubmoduleInfo> {
  const { data } = await api.post<SubmoduleInfo>(`/repos/${repoId}/submodules`, { name, path, url });
  return data;
}

export async function deleteSubmodule(repoId: string, name: string): Promise<void> {
  await api.delete(`/repos/${repoId}/submodules/${name}`);
}

// Shortlog
export async function getShortlog(repoId: string): Promise<ShortlogResponse> {
  const { data } = await api.get<ShortlogResponse>(`/repos/${repoId}/shortlog`);
  return data;
}

// Archive
export async function downloadArchive(repoId: string, format: 'tar' | 'zip' = 'tar'): Promise<Blob> {
  const { data } = await api.get<Blob>(`/repos/${repoId}/archive`, {
    params: { format },
    responseType: 'blob',
  });
  return data;
}

// File history (log filtered by path)
export async function getFileHistory(
  repoId: string,
  path: string,
  limit = 50,
): Promise<FileHistoryResponse> {
  const { data } = await api.get<FileHistoryResponse>(`/repos/${repoId}/log`, {
    params: { path, limit },
  });
  return data;
}

// Compare two refs
export async function compareCommits(
  repoId: string,
  base: string,
  head: string,
): Promise<CompareResponse> {
  const { data } = await api.get<CompareResponse>(`/repos/${repoId}/compare`, {
    params: { base, head },
  });
  return data;
}

// Dependencies
export async function getDependencies(repoId: string): Promise<DependencyReport> {
  const { data } = await api.get<DependencyReport>(`/repos/${repoId}/dependencies`);
  return data;
}

export async function createDependencyUpdates(
  repoId: string,
  updates?: Pick<DependencyProposal, 'dependency' | 'file' | 'to_version'>[],
): Promise<DependencyUpdateResponse> {
  const { data } = await api.post<DependencyUpdateResponse>(
    `/repos/${repoId}/dependencies/update`,
    updates !== undefined ? { updates } : {},
  );
  return data;
}

export async function listDependencyProposals(
  repoId: string,
): Promise<DependencyProposalsResponse> {
  const { data } = await api.get<DependencyProposalsResponse>(
    `/repos/${repoId}/dependencies/proposals`,
  );
  return data;
}

export async function deleteDependencyProposal(
  repoId: string,
  branch: string,
): Promise<void> {
  await api.delete(
    `/repos/${repoId}/dependencies/proposals/${encodeURIComponent(branch)}`,
  );
}

export async function mergeDependencyProposal(
  repoId: string,
  branch: string,
): Promise<DepMergeResponse> {
  const { data } = await api.post<DepMergeResponse>(
    `/repos/${repoId}/dependencies/proposals/${encodeURIComponent(branch)}/merge`,
  );
  return data;
}

// Action Config CRUD (per-action)
export async function getActionConfig(
  repoId: string,
  name: string,
): Promise<ActionConfigResponse> {
  const { data } = await api.get<ActionConfigResponse>(
    `/repos/${repoId}/actions/config/${encodeURIComponent(name)}`,
  );
  return data;
}

export async function putActionConfig(
  repoId: string,
  name: string,
  config: ActionConfigDetail,
): Promise<void> {
  await api.put(
    `/repos/${repoId}/actions/config/${encodeURIComponent(name)}`,
    config,
  );
}

export async function deleteActionConfig(
  repoId: string,
  name: string,
): Promise<void> {
  await api.delete(
    `/repos/${repoId}/actions/config/${encodeURIComponent(name)}`,
  );
}

// Documentation
export async function getDocsIndex(): Promise<DocIndexResponse> {
  const { data } = await api.get<DocIndexResponse>('/docs');
  return data;
}

export async function searchDocs(query: string): Promise<DocSearchResponse> {
  const { data } = await api.get<DocSearchResponse>('/docs/search', {
    params: { q: query },
  });
  return data;
}

export async function getDocSection(
  category: string,
  section: string,
): Promise<DocSectionResponse> {
  const { data } = await api.get<DocSectionResponse>(
    `/docs/${encodeURIComponent(category)}/${encodeURIComponent(section)}`,
  );
  return data;
}

// Access Control
export async function listAccess(repoId: string): Promise<ListAccessResponse> {
  const { data } = await api.get<ListAccessResponse>(`/repos/${repoId}/access`);
  return data;
}

export async function grantAccess(repoId: string, payload: GrantAccessPayload): Promise<UserAccessInfo> {
  const { data } = await api.post<UserAccessInfo>(`/repos/${repoId}/access/grant`, payload);
  return data;
}

export async function revokeAccess(repoId: string, fingerprint: string): Promise<void> {
  await api.post(`/repos/${repoId}/access/revoke`, { fingerprint });
}

export async function setRole(repoId: string, fingerprint: string, payload: SetRolePayload): Promise<UserAccessInfo> {
  const { data } = await api.put<UserAccessInfo>(`/repos/${repoId}/access/${encodeURIComponent(fingerprint)}/role`, payload);
  return data;
}

export async function listBranchProtection(repoId: string): Promise<BranchProtectionInfo[]> {
  const { data } = await api.get<BranchProtectionInfo[]>(`/repos/${repoId}/branch-protect`);
  return data;
}

export async function setBranchProtection(repoId: string, branch: string, payload: SetBranchProtectionPayload): Promise<BranchProtectionInfo> {
  const { data } = await api.put<BranchProtectionInfo>(`/repos/${repoId}/branch-protect/${encodeURIComponent(branch)}`, payload);
  return data;
}

export async function removeBranchProtection(repoId: string, branch: string): Promise<void> {
  await api.delete(`/repos/${repoId}/branch-protect/${encodeURIComponent(branch)}`);
}

// PR Reviews
export async function listReviews(repoId: string, prNumber: number): Promise<PrReview[]> {
  const { data } = await api.get<PrReview[]>(`/repos/${repoId}/pulls/by-number/${prNumber}/reviews`);
  return data;
}

export async function createReview(repoId: string, prNumber: number, payload: CreateReviewPayload): Promise<PrReview> {
  const { data } = await api.post<PrReview>(`/repos/${repoId}/pulls/by-number/${prNumber}/reviews`, payload);
  return data;
}

// PR Comments
export async function listComments(repoId: string, prNumber: number): Promise<PrComment[]> {
  const { data } = await api.get<PrComment[]>(`/repos/${repoId}/pulls/by-number/${prNumber}/comments`);
  return data;
}

export async function createComment(repoId: string, prNumber: number, payload: CreateCommentPayload): Promise<PrComment> {
  const { data } = await api.post<PrComment>(`/repos/${repoId}/pulls/by-number/${prNumber}/comments`, payload);
  return data;
}

export async function updateComment(repoId: string, prNumber: number, commentId: number, body: string): Promise<PrComment> {
  const { data } = await api.patch<PrComment>(`/repos/${repoId}/pulls/by-number/${prNumber}/comments/${commentId}`, { body });
  return data;
}

export async function deleteComment(repoId: string, prNumber: number, commentId: number): Promise<void> {
  await api.delete(`/repos/${repoId}/pulls/by-number/${prNumber}/comments/${commentId}`);
}

// Archive (with commit ref support)
export async function downloadArchiveWithRef(
  repoId: string,
  format: 'tar' | 'zip' = 'tar',
  commit?: string,
): Promise<Blob> {
  const params: Record<string, string> = { format };
  if (commit) params.commit = commit;
  const { data } = await api.post<Blob>(`/repos/${repoId}/archive`, null, {
    params,
    responseType: 'blob',
  });
  return data;
}

// Git Import / Export
export interface GitImportResponse {
  commits_imported: number;
  message: string;
}

export interface GitExportResponse {
  commits_exported: number;
  message: string;
}

export async function gitImport(repoId: string, path: string): Promise<GitImportResponse> {
  const { data } = await api.post<GitImportResponse>(`/repos/${repoId}/git-import`, { path });
  return data;
}

export async function gitExport(repoId: string, path: string): Promise<GitExportResponse> {
  const { data } = await api.post<GitExportResponse>(`/repos/${repoId}/git-export`, { path });
  return data;
}

// Key-Based Auth
export async function getAuthChallenge(): Promise<ChallengeResponse> {
  const { data } = await api.get<ChallengeResponse>('/auth/challenge');
  return data;
}

export async function keyAuth(payload: KeyAuthPayload): Promise<TokenResponse> {
  const { data } = await api.post<TokenResponse>('/auth/key-auth', payload);
  return data;
}

// LLM Integration

export async function getLlmConfig(repoId: string): Promise<LlmConfigResponse> {
  const { data } = await api.get<LlmConfigResponse>(`/repos/${repoId}/llm/config`);
  return data;
}

export async function updateLlmConfig(
  repoId: string,
  payload: UpdateLlmConfigPayload,
): Promise<LlmConfigResponse> {
  const { data } = await api.put<LlmConfigResponse>(`/repos/${repoId}/llm/config`, payload);
  return data;
}

export async function getLlmHealth(repoId?: string): Promise<LlmHealthResponse> {
  const params = repoId ? { repo_id: repoId } : {};
  const { data } = await api.get<LlmHealthResponse>('/llm/health', { params });
  return data;
}

export async function generatePrDescription(
  repoId: string,
  prNumber: number,
): Promise<LlmDescriptionResponse> {
  const { data } = await api.post<LlmDescriptionResponse>(
    `/repos/${repoId}/llm/generate-pr-description/${prNumber}`,
  );
  return data;
}

/**
 * Creates an async generator that streams SSE events from an LLM endpoint.
 * Uses native fetch for streaming (axios buffers responses).
 */
export async function* streamLlmResponse(
  path: string,
  body?: Record<string, unknown>,
  signal?: AbortSignal,
): AsyncGenerator<string, void, unknown> {
  const token = localStorage.getItem('ovc_token');
  const response = await fetch(`/api/v1${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
    signal,
  });

  if (!response.ok) {
    const errorBody = await response.text().catch(() => response.statusText);
    throw new Error(`LLM request failed (${response.status}): ${errorBody}`);
  }

  if (!response.body) return;

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let currentEventType: string | null = null;

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';

      for (const line of lines) {
        const trimmed = line.trim();
        if (trimmed === '') {
          // Empty line resets event type per SSE spec.
          currentEventType = null;
          continue;
        }
        if (trimmed.startsWith('event: ')) {
          currentEventType = trimmed.slice(7);
          if (currentEventType === 'done') {
            return;
          }
          continue;
        }
        if (trimmed.startsWith('data: ')) {
          const data = trimmed.slice(6);
          if (currentEventType === 'error') {
            throw new Error(data || 'LLM stream error');
          }
          // Progress events are yielded with a special prefix so the
          // consumer can distinguish them from content deltas.
          if (currentEventType === 'progress') {
            if (data) yield `\x00progress:${data}`;
            continue;
          }
          if (data) yield data;
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

export function streamCommitMessage(repoId: string, signal?: AbortSignal) {
  return streamLlmResponse(`/repos/${repoId}/llm/generate-commit-msg`, undefined, signal);
}

export function streamPrReview(repoId: string, prNumber: number, signal?: AbortSignal) {
  return streamLlmResponse(`/repos/${repoId}/llm/review-pr/${prNumber}`, undefined, signal);
}

export function streamExplainDiff(repoId: string, diff: string, signal?: AbortSignal) {
  return streamLlmResponse(`/repos/${repoId}/llm/explain-diff`, { diff }, signal);
}
