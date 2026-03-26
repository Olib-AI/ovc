export interface RepoInfo {
  id: string;
  name: string;
  path: string;
  head: string;
  repo_stats: RepoStats;
}

export interface RepoStats {
  total_commits: number;
  total_branches: number;
  total_tags: number;
  tracked_files: number;
}

export interface FileTreeEntry {
  name: string;
  path: string;
  entry_type: string;
  size: number;
}

export interface FileContent {
  path: string;
  content: string;
  is_binary: boolean;
  size_bytes: number;
}

export interface CommitInfo {
  id: string;
  short_id: string;
  message: string;
  author: CommitAuthor;
  authored_at: string;
  parent_ids: string[];
  signature_status: 'verified' | 'unverified' | 'unsigned';
  signer_fingerprint: string | null;
  signer_identity: string | null;
}

export interface CommitAuthor {
  name: string;
  email: string;
}

export interface CommitLog {
  commits: CommitInfo[];
}

export interface DiffResponse {
  files: FileDiff[];
  stats: DiffStats;
}

export interface DiffStats {
  files_changed: number;
  additions: number;
  deletions: number;
}

export interface FileDiff {
  path: string;
  status: string;
  additions?: number;
  deletions?: number;
  hunks: DiffHunk[];
}

export interface DiffHunk {
  old_start: number;
  old_count: number;
  new_start: number;
  new_count: number;
  lines: DiffLine[];
}

export interface DiffLine {
  kind: 'context' | 'addition' | 'deletion';
  content: string;
}

export interface StatusResponse {
  branch: string;
  staged: FileStatusEntry[];
  unstaged: FileStatusEntry[];
  untracked: string[];
  /** Whether the server has access to a working directory.
   *  When false, unstaged and untracked will always be empty arrays. */
  has_workdir: boolean;
}

export interface FileStatusEntry {
  path: string;
  status: string;
}

export interface BranchInfo {
  name: string;
  commit_id: string;
  is_current: boolean;
}

export interface TagInfo {
  name: string;
  commit_id: string;
  message?: string;
}

export interface StashEntryInfo {
  index: number;
  message: string;
  commit_id: string;
  base_commit_id: string;
  timestamp: number;
}

export interface SyncStatusResponse {
  status: string;
  remote: string | null;
  version: number | null;
}

export interface MergeResponse {
  status: string;
  commit_id: string | null;
  conflict_files: string[];
  message: string;
}

export interface TokenResponse {
  token: string;
  expires_at: string;
}

export interface GcResponse {
  objects_before: number;
  objects_after: number;
  objects_removed: number;
  bytes_before: number;
  bytes_after: number;
  bytes_freed: number;
}

export interface PushResponse {
  chunks_uploaded: number;
  bytes_uploaded: number;
}

export interface PullResponse {
  chunks_downloaded: number;
  bytes_downloaded: number;
}

export interface RebaseResponse {
  status: string;
  new_tip: string | null;
  replayed_count: number;
  replayed: [string, string][];
  conflict_files: string[];
}

export interface CherryPickResponse {
  new_commit_id: string;
  source_commit_id: string;
}

export interface RevertResponse {
  new_commit_id: string;
  source_commit_id: string;
}

export interface RemoteInfo {
  name: string;
  url: string;
  backend_type: string;
}

// Actions

export interface ActionLastRun {
  status: string;
  duration_ms: number;
  timestamp: string;
}

export interface ActionInfo {
  name: string;
  display_name: string;
  category: string;
  language: string | null;
  tool: string | null;
  triggers: string[];
  last_run: ActionLastRun | null;
}

export interface ActionListResponse {
  actions: ActionInfo[];
}

export interface RunActionsRequest {
  names?: string[];
  trigger?: string;
  fix?: boolean;
  changed_paths?: string[];
}

export interface ActionResultResponse {
  name: string;
  display_name: string;
  category: string;
  status: string;
  exit_code: number | null;
  stdout: string;
  stderr: string;
  duration_ms: number;
  started_at: string;
  finished_at: string;
  docker_used: boolean;
}

export interface RunActionsResponse {
  run_id: string;
  trigger: string;
  overall_status: string;
  total_duration_ms: number;
  results: ActionResultResponse[];
}

export interface DetectedLanguageInfo {
  language: string;
  confidence: string;
  marker_file: string;
  root_dir: string;
}

export interface DetectionResponse {
  languages: DetectedLanguageInfo[];
  suggested_config: unknown;
}

export interface ActionRunSummary {
  run_id: string;
  trigger: string;
  timestamp: string;
  overall_status: string;
  total_duration_ms: number;
  action_count: number;
  passed_count: number;
  failed_count: number;
}

export interface ActionHistoryListResponse {
  runs: ActionRunSummary[];
}

export interface InitActionsRequest {
  force: boolean;
}

export interface ActionsConfigResponse {
  content: string;
  exists: boolean;
}

export interface SecretsListResponse {
  names: string[];
}

export interface SecretPutResponse {
  ok: boolean;
}

export interface SecretDeleteResponse {
  ok: boolean;
}

export interface DockerStatusResponse {
  available: boolean;
  version: string | null;
  enabled: boolean;
  image: string;
  pull_policy: string;
  reason: string | null;
}

// Blame

export interface BlameLine {
  commit_id: string;
  author: string;
  timestamp: number;
  line_number: number;
  content: string;
}

export interface BlameResponse {
  file: string;
  lines: BlameLine[];
}

// Search

export interface SearchMatch {
  path: string;
  line_number: number;
  line: string;
}

export interface SearchResponse {
  query: string;
  total_matches: number;
  truncated: boolean;
  results: SearchMatch[];
}

// Notes

export interface NoteResponse {
  commit_id: string;
  message: string;
}

// Reflog

export interface ReflogEntry {
  ref_name: string;
  old_value: string | null;
  new_value: string;
  identity_name: string;
  identity_email: string;
  message: string;
  timestamp: number;
}

// Reset

export type ResetMode = 'soft' | 'mixed' | 'hard';

export interface ResetRequest {
  commit_id?: string;
  mode: ResetMode;
}

export interface ResetResponse {
  commit_id: string;
  mode: string;
}

// Describe

export interface DescribeResponse {
  commit_id: string;
  description: string;
}

// Submodule

export interface SubmoduleInfo {
  name: string;
  path: string;
  url: string;
  ovc_file: string;
  pinned_sequence: number;
  /** Backend-provided status for the submodule. "configured" means stored but not checked out. */
  status?: string;
}

// Clean

export interface CleanResponse {
  deleted: string[];
}

// Shortlog / contributor stats

export interface ShortlogAuthor {
  name: string;
  email: string;
  count: number;
}

export interface ShortlogResponse {
  authors: ShortlogAuthor[];
}

// File history (log filtered by path)

export interface FileHistoryResponse {
  commits: CommitInfo[];
}

// Compare

export interface CompareResponse {
  base: string;
  head: string;
  files: FileDiff[];
  stats: DiffStats;
}

// Pull Request / Branch Comparison

export interface PullRequestView {
  branch: string;
  base: string;
  commits: CommitInfo[];
  diff: DiffResponse;
  mergeable: boolean;
  conflict_files: string[];
  ahead_by: number;
  behind_by: number;
}

// PR Lifecycle

export type PullRequestState = 'open' | 'closed' | 'merged';

export interface PullRequestSummary {
  number: number;
  title: string;
  state: PullRequestState;
  source_branch: string;
  target_branch: string;
  author: string;
  created_at: string;
  updated_at: string;
  merged_at?: string;
  checks?: PrChecks | null;
}

export interface PullRequestFull extends PullRequestSummary {
  description: string;
  merge_commit?: string;
  reviews?: PrReview[];
  comments?: PrComment[];
  required_approvals?: number;
}

export interface CreatePullRequestPayload {
  title: string;
  description?: string;
  source_branch: string;
  target_branch?: string;
  author?: string;
}

export interface UpdatePullRequestPayload {
  title?: string;
  description?: string;
  state?: PullRequestState;
}

export type PrMergeStrategy = 'merge' | 'squash' | 'rebase';

export type PrCheckStatus = 'pending' | 'passing' | 'failing';

export interface PrCheckResult {
  name: string;
  display_name: string;
  category: string;
  status: string;
  duration_ms: number;
  docker_used: boolean;
}

export interface PrChecks {
  status: PrCheckStatus;
  results: PrCheckResult[];
  ran_at: string;
}

// Dependencies

export type UpdateType = 'major' | 'minor' | 'patch' | 'up_to_date' | 'unknown';

export interface DependencyProposal {
  branch: string;
  dependency: string;
  file: string;
  from_version: string;
  to_version: string;
  update_type: string;
  mergeable: boolean;
  conflict_files: string[];
  error?: string;
}

export interface DependencyUpdateResponse {
  proposals: DependencyProposal[];
  created: number;
  mergeable: number;
  conflicting: number;
}

export interface ProposalInfo {
  branch: string;
  commit_id?: string;
  dependency?: string;
  file?: string;
  mergeable: boolean;
  conflict_files: string[];
}

export interface DependencyProposalsResponse {
  proposals: ProposalInfo[];
  total: number;
}

export interface DepMergeResponse {
  status: string;
  message: string;
  commit_id?: string;
  conflict_files?: string[];
}

export interface DependencyStatus {
  name: string;
  current_version: string;
  latest_version: string;
  update_type: UpdateType;
  dev: boolean;
}

export interface ManifestReport {
  file: string;
  package_manager: string;
  dependencies: DependencyStatus[];
}

export interface DependencyReport {
  manifests: ManifestReport[];
  total_updates: number;
  major_updates: number;
  minor_updates: number;
  patch_updates: number;
}

// Action Config CRUD (per-action)

export interface ActionConfigDetail {
  name: string;
  command: string;
  display_name?: string;
  trigger?: string[];
  category?: string;
  timeout?: number;
  working_dir?: string;
  condition_paths?: string[];
  depends_on?: string[];
  builtin?: string;
  auto_fix?: boolean;
  continue_on_error?: boolean;
  env?: Record<string, string>;
  docker?: boolean;
}

export interface ActionConfigResponse {
  action: ActionConfigDetail;
}

// Documentation

export interface DocSection {
  id: string;
  title: string;
  content: string;
}

export interface DocSectionSummary {
  id: string;
  title: string;
  tags: string[];
}

export interface DocCategory {
  id: string;
  title: string;
  sections: DocSectionSummary[];
}

export interface DocIndexResponse {
  categories: DocCategory[];
}

export interface DocSectionResponse {
  category: string;
  section: string;
  title: string;
  content: string;
}

export interface DocSearchResult {
  category: string;
  section: string;
  title: string;
  snippet: string;
  score: number;
}

export interface DocSearchResponse {
  query: string;
  results: DocSearchResult[];
}

// Access Control

export type AccessRole = 'read' | 'write' | 'admin' | 'owner';

export interface UserAccessInfo {
  fingerprint: string;
  role: string;
  identity: string | null;
  added_at: string;
  added_by: string;
  /** True if this user created the repo. Their key cannot be revoked. */
  is_repo_creator: boolean;
}

export interface ListAccessResponse {
  users: UserAccessInfo[];
}

export interface GrantAccessPayload {
  public_key_pem?: string;
  fingerprint?: string;
  role: string;
}

export interface SetRolePayload {
  role: string;
}

export interface BranchProtectionInfo {
  branch: string;
  required_approvals: number;
  require_ci_pass: boolean;
  allowed_merge_roles: string[];
  allowed_push_roles: string[];
}

export interface SetBranchProtectionPayload {
  required_approvals?: number;
  require_ci_pass?: boolean;
  allowed_merge_roles?: string[];
  allowed_push_roles?: string[];
}

// PR Reviews & Comments

export type ReviewState = 'approved' | 'changes_requested' | 'commented';

export interface PrReview {
  id: number;
  author: string;
  author_identity: string | null;
  state: ReviewState;
  body: string;
  created_at: string;
  signature: string | null;
  verified: boolean;
}

export interface PrComment {
  id: number;
  author: string;
  author_identity: string | null;
  body: string;
  file_path: string | null;
  line_number: number | null;
  created_at: string;
  updated_at: string;
}

export interface CreateReviewPayload {
  state: ReviewState;
  body: string;
  signature?: string;
}

export interface CreateCommentPayload {
  body: string;
  file_path?: string;
  line_number?: number;
}

// Repo Config

export interface RepoConfigResponse {
  user_name: string;
  user_email: string;
  default_branch: string;
}

export interface UpdateRepoConfigPayload {
  user_name?: string;
  user_email?: string;
  default_branch?: string;
}

// Key-Based Auth

export interface ChallengeResponse {
  challenge: string;
  expires_in: number;
}

export interface KeyAuthPayload {
  repo_id: string;
  fingerprint: string;
  challenge: string;
  signature: string;
}

// LLM Integration

export interface LlmFeatureToggles {
  commit_message: boolean;
  pr_description: boolean;
  pr_review: boolean;
  explain_diff: boolean;
}

export interface LlmRepoConfig {
  base_url?: string;
  model?: string;
  enabled_features: LlmFeatureToggles;
}

export interface LlmConfigResponse {
  server_enabled: boolean;
  base_url?: string;
  model?: string;
  max_context_tokens?: number;
  temperature?: number;
  enabled_features?: LlmFeatureToggles;
}

export interface UpdateLlmConfigPayload {
  base_url?: string;
  model?: string;
  max_context_tokens?: number;
  temperature?: number;
  enabled_features?: LlmFeatureToggles;
}

export interface LlmHealthResponse {
  configured: boolean;
  reachable: boolean;
  model?: string;
  base_url?: string;
}

export interface LlmDescriptionResponse {
  description: string;
}
