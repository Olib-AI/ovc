import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { Shield, ShieldCheck, UserPlus, Trash2, GitBranch, Lock } from 'lucide-react';
import {
  useListAccess,
  useGrantAccess,
  useRevokeAccess,
  useSetRole,
  useListBranchProtection,
  useSetBranchProtection,
  useRemoveBranchProtection,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import type { UserAccessInfo, BranchProtectionInfo } from '../api/types.ts';

const ROLE_BADGE_CLASSES: Record<string, string> = {
  owner: 'bg-purple-500/20 text-purple-400 border-purple-500/30',
  admin: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
  write: 'bg-green-500/20 text-green-400 border-green-500/30',
  read: 'bg-gray-500/20 text-gray-400 border-gray-500/30',
};

const ROLES = ['read', 'write', 'admin', 'owner'] as const;

function timeAgo(dateStr: string): string {
  const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

function RoleBadge({ role }: { role: string }) {
  const classes = ROLE_BADGE_CLASSES[role] ?? ROLE_BADGE_CLASSES.read;
  return (
    <span className={`inline-block rounded border px-1.5 py-0.5 text-[11px] font-medium ${classes}`}>
      {role}
    </span>
  );
}

function UserAccessCard({
  user,
  repoId,
}: {
  user: UserAccessInfo;
  repoId: string;
}) {
  const toast = useToast();
  const setRoleMutation = useSetRole(repoId);
  const revokeMutation = useRevokeAccess(repoId);
  const [selectedRole, setSelectedRole] = useState(user.role);
  const [confirmRevoke, setConfirmRevoke] = useState(false);

  function handleRoleChange(newRole: string) {
    setSelectedRole(newRole);
    setRoleMutation.mutate(
      { fingerprint: user.fingerprint, role: newRole },
      {
        onSuccess: () => toast.success(`Role updated to ${newRole}`),
        onError: (err: Error) => {
          toast.error(err.message);
          setSelectedRole(user.role);
        },
      },
    );
  }

  function handleRevoke() {
    revokeMutation.mutate(user.fingerprint, {
      onSuccess: () => {
        toast.success('Access revoked');
        setConfirmRevoke(false);
      },
      onError: (err: Error) => toast.error(err.message),
    });
  }

  return (
    <div className="rounded-md border border-border bg-navy-950/50 p-3 space-y-2">
      {/* Row 1: Identity + badges */}
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-text-primary truncate">
            {user.identity ?? <span className="text-text-muted italic">Unknown identity</span>}
          </div>
          <div className="text-[11px] font-mono text-text-muted truncate" title={user.fingerprint}>
            {user.fingerprint}
          </div>
        </div>
        <div className="flex items-center gap-1.5 flex-shrink-0">
          <RoleBadge role={user.role} />
          {user.is_repo_creator && (
            <span className="inline-flex items-center gap-1 rounded border border-accent/30 bg-accent/10 px-1.5 py-0.5 text-[10px] font-semibold text-accent whitespace-nowrap">
              <Lock size={9} />
              Creator
            </span>
          )}
        </div>
      </div>

      {/* Row 2: Added info */}
      <div className="text-[11px] text-text-muted">
        Added {timeAgo(user.added_at)}
        {user.added_by !== user.fingerprint && (
          <span> by <span className="font-mono">{user.added_by.slice(0, 16)}…</span></span>
        )}
      </div>

      {/* Row 3: Actions */}
      {user.is_repo_creator ? (
        <div className="flex items-center gap-1.5 text-[11px] text-text-muted">
          <Lock size={11} className="text-accent/60" />
          <span>Protected — this key encrypts the repository</span>
        </div>
      ) : (
        <div className="flex items-center gap-2 flex-wrap">
          <select
            value={selectedRole}
            onChange={(e) => handleRoleChange(e.target.value)}
            disabled={setRoleMutation.isPending}
            className="rounded border border-border bg-navy-950 px-2 py-1 text-xs text-text-primary focus:border-accent focus:outline-none disabled:opacity-50"
          >
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
          {!confirmRevoke ? (
            <button
              onClick={() => setConfirmRevoke(true)}
              className="rounded border border-status-deleted/30 px-2 py-1 text-[11px] font-medium text-status-deleted hover:bg-status-deleted/10 disabled:opacity-50"
              title="Revoke access"
            >
              Revoke
            </button>
          ) : (
            <div className="flex items-center gap-1">
              <button
                onClick={handleRevoke}
                disabled={revokeMutation.isPending}
                className="rounded bg-status-deleted px-2 py-1 text-[11px] font-medium text-white hover:opacity-90 disabled:opacity-50"
              >
                {revokeMutation.isPending ? 'Revoking…' : 'Confirm Revoke'}
              </button>
              <button
                onClick={() => setConfirmRevoke(false)}
                className="rounded px-2 py-1 text-[11px] text-text-muted hover:text-text-primary"
              >
                Cancel
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function GrantAccessForm({ repoId }: { repoId: string }) {
  const toast = useToast();
  const grantMutation = useGrantAccess(repoId);
  const [keyPem, setKeyPem] = useState('');
  const [role, setRole] = useState<string>('write');

  function handleGrant() {
    const trimmed = keyPem.trim();
    if (!trimmed) return;

    const isPem = trimmed.startsWith('-----BEGIN');
    const payload = isPem
      ? { public_key_pem: trimmed, role }
      : { fingerprint: trimmed, role };

    grantMutation.mutate(payload, {
      onSuccess: () => {
        toast.success('Access granted');
        setKeyPem('');
        setRole('write');
      },
      onError: (err: Error) => toast.error(err.message),
    });
  }

  return (
    <div className="mt-4 space-y-3 rounded border border-border bg-navy-950 p-3">
      <div className="flex items-center gap-1.5 text-xs font-medium text-text-primary">
        <UserPlus size={13} />
        Grant Access
      </div>
      <textarea
        value={keyPem}
        onChange={(e) => setKeyPem(e.target.value)}
        placeholder="Paste public key PEM or fingerprint..."
        rows={4}
        className="w-full rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none font-mono"
      />
      <div className="flex items-center gap-3">
        <select
          value={role}
          onChange={(e) => setRole(e.target.value)}
          className="rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary focus:border-accent focus:outline-none"
        >
          {ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button
          onClick={handleGrant}
          disabled={!keyPem.trim() || grantMutation.isPending}
          className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-white hover:bg-accent-light disabled:opacity-50"
        >
          {grantMutation.isPending ? 'Granting...' : 'Grant Access'}
        </button>
      </div>
    </div>
  );
}

const ROLE_BADGE_CLASSES_SMALL: Record<string, string> = {
  owner: 'bg-purple-500/15 text-purple-400',
  admin: 'bg-blue-500/15 text-blue-400',
  write: 'bg-green-500/15 text-green-400',
  read: 'bg-gray-500/15 text-gray-400',
};

function RolePillList({ roles, label }: { roles: string[]; label: string }) {
  if (roles.length === 0) return null;
  return (
    <div className="flex items-center gap-1 flex-wrap">
      <span className="text-[10px] text-text-muted">{label}:</span>
      {roles.map((r) => (
        <span
          key={r}
          className={`rounded px-1.5 py-0 text-[10px] font-medium leading-4 ${ROLE_BADGE_CLASSES_SMALL[r] ?? 'bg-gray-500/15 text-gray-400'}`}
        >
          {r}
        </span>
      ))}
    </div>
  );
}

function BranchProtectionRule({
  rule,
  repoId,
}: {
  rule: BranchProtectionInfo;
  repoId: string;
}) {
  const toast = useToast();
  const removeMutation = useRemoveBranchProtection(repoId);
  const [confirmDelete, setConfirmDelete] = useState(false);

  function handleDelete() {
    removeMutation.mutate(rule.branch, {
      onSuccess: () => {
        toast.success(`Protection removed from ${rule.branch}`);
        setConfirmDelete(false);
      },
      onError: (err: Error) => toast.error(err.message),
    });
  }

  return (
    <div className="space-y-2 rounded border border-border bg-navy-950 px-3 py-2.5">
      {/* Top row: branch name + approval/CI badges + delete */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex items-center gap-1.5">
            <GitBranch size={13} className="text-accent flex-shrink-0" />
            <span className="text-xs font-mono font-semibold text-text-primary">{rule.branch}</span>
          </div>
          <span className="rounded border border-border bg-navy-900 px-1.5 py-0.5 text-[11px] text-text-secondary">
            {rule.required_approvals} approval{rule.required_approvals !== 1 ? 's' : ''} required
          </span>
          {rule.require_ci_pass && (
            <span className="inline-flex items-center gap-1 rounded border border-green-500/30 bg-green-500/20 px-1.5 py-0.5 text-[11px] font-medium text-green-400">
              <ShieldCheck size={11} />
              CI required
            </span>
          )}
        </div>
        <div className="flex-shrink-0">
          {!confirmDelete ? (
            <button
              onClick={() => setConfirmDelete(true)}
              className="rounded p-1 text-text-muted transition-colors hover:bg-status-deleted/10 hover:text-status-deleted"
              title="Remove protection"
            >
              <Trash2 size={13} />
            </button>
          ) : (
            <div className="flex items-center gap-1">
              <button
                onClick={handleDelete}
                disabled={removeMutation.isPending}
                className="rounded bg-red-600 px-2 py-1 text-[11px] font-medium text-white hover:bg-red-500 disabled:opacity-50"
              >
                {removeMutation.isPending ? 'Removing...' : 'Confirm'}
              </button>
              <button
                onClick={() => setConfirmDelete(false)}
                className="rounded px-2 py-1 text-[11px] text-text-muted hover:text-text-primary"
              >
                Cancel
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Second row: allowed roles (only shown if non-empty) */}
      {(rule.allowed_merge_roles.length > 0 || rule.allowed_push_roles.length > 0) && (
        <div className="flex flex-wrap gap-3 border-t border-border/50 pt-2">
          <RolePillList roles={rule.allowed_merge_roles} label="Merge" />
          <RolePillList roles={rule.allowed_push_roles} label="Push" />
        </div>
      )}
    </div>
  );
}

const ALL_ROLES = ['read', 'write', 'admin', 'owner'] as const;

/**
 * Multi-select role checklist used for allowed_merge_roles / allowed_push_roles.
 * An empty selection means "no restriction" (all roles are implicitly allowed).
 */
function RoleMultiSelect({
  label,
  selected,
  onChange,
}: {
  label: string;
  selected: string[];
  onChange: (roles: string[]) => void;
}) {
  function toggle(role: string) {
    onChange(
      selected.includes(role) ? selected.filter((r) => r !== role) : [...selected, role],
    );
  }

  return (
    <div className="space-y-1">
      <div className="text-[11px] font-medium text-text-muted">{label} (empty = any role)</div>
      <div className="flex flex-wrap gap-1.5">
        {ALL_ROLES.map((r) => {
          const active = selected.includes(r);
          const classes = ROLE_BADGE_CLASSES_SMALL[r] ?? 'bg-gray-500/15 text-gray-400';
          return (
            <button
              key={r}
              type="button"
              onClick={() => toggle(r)}
              className={`rounded border px-2 py-0.5 text-[11px] font-medium transition-opacity ${
                active
                  ? `${classes} border-current opacity-100`
                  : 'border-border opacity-40 hover:opacity-70'
              }`}
            >
              {r}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function AddBranchProtectionForm({ repoId }: { repoId: string }) {
  const toast = useToast();
  const setProtectionMutation = useSetBranchProtection(repoId);
  const [branch, setBranch] = useState('');
  const [requiredApprovals, setRequiredApprovals] = useState(1);
  const [requireCi, setRequireCi] = useState(false);
  const [allowedMergeRoles, setAllowedMergeRoles] = useState<string[]>([]);
  const [allowedPushRoles, setAllowedPushRoles] = useState<string[]>([]);

  function handleSubmit() {
    const trimmed = branch.trim();
    if (!trimmed) return;

    setProtectionMutation.mutate(
      {
        branch: trimmed,
        payload: {
          required_approvals: requiredApprovals,
          require_ci_pass: requireCi,
          ...(allowedMergeRoles.length > 0 ? { allowed_merge_roles: allowedMergeRoles } : {}),
          ...(allowedPushRoles.length > 0 ? { allowed_push_roles: allowedPushRoles } : {}),
        },
      },
      {
        onSuccess: () => {
          toast.success(`Branch "${trimmed}" protected`);
          setBranch('');
          setRequiredApprovals(1);
          setRequireCi(false);
          setAllowedMergeRoles([]);
          setAllowedPushRoles([]);
        },
        onError: (err: Error) => toast.error(err.message),
      },
    );
  }

  return (
    <div className="mt-4 space-y-3 rounded border border-border bg-navy-950 p-3">
      <div className="flex items-center gap-1.5 text-xs font-medium text-text-primary">
        <Lock size={13} />
        Add Branch Protection
      </div>
      <input
        value={branch}
        onChange={(e) => setBranch(e.target.value)}
        placeholder="Branch name (e.g. main)"
        className="w-full rounded border border-border bg-navy-950 px-3 py-2 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
      />
      <div className="flex flex-wrap items-center gap-4">
        <label className="flex items-center gap-2 text-xs text-text-secondary">
          Required approvals
          <input
            type="number"
            min={0}
            max={10}
            value={requiredApprovals}
            onChange={(e) => setRequiredApprovals(Number(e.target.value))}
            className="w-16 rounded border border-border bg-navy-950 px-2 py-1 text-sm text-text-primary focus:border-accent focus:outline-none"
          />
        </label>
        <label className="flex items-center gap-2 text-xs text-text-secondary">
          <input
            type="checkbox"
            checked={requireCi}
            onChange={(e) => setRequireCi(e.target.checked)}
            className="accent-accent"
          />
          Require CI
        </label>
      </div>

      <RoleMultiSelect label="Who can merge" selected={allowedMergeRoles} onChange={setAllowedMergeRoles} />
      <RoleMultiSelect label="Who can push" selected={allowedPushRoles} onChange={setAllowedPushRoles} />

      <button
        onClick={handleSubmit}
        disabled={!branch.trim() || setProtectionMutation.isPending}
        className="rounded bg-accent px-3 py-1.5 text-sm font-medium text-white hover:bg-accent-light disabled:opacity-50"
      >
        {setProtectionMutation.isPending ? 'Protecting...' : 'Protect Branch'}
      </button>
    </div>
  );
}

function AccessPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} — Access — OVC`);
  const toast = useToast();

  const { data: accessData, isLoading: accessLoading } = useListAccess(repoId);
  const { data: protectionRules, isLoading: protectionLoading } = useListBranchProtection(repoId);

  // Suppress unused variable lint — toast is used in child components and kept
  // here for consistency with the codebase pattern (SettingsPage).
  void toast;

  if (!repoId) return null;

  if (accessLoading || protectionLoading) {
    return <LoadingSpinner className="h-full" message="Loading access settings..." />;
  }

  const users: UserAccessInfo[] = accessData?.users ?? [];
  const rules: BranchProtectionInfo[] = protectionRules ?? [];

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Shield size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Access Management</h1>
      </div>

      <div className="mx-auto max-w-xl space-y-6 p-6">
        {/* User Access */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center gap-2 mb-3">
            <Shield size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">User Access</h2>
          </div>

          {users.length > 0 ? (
            <div className="space-y-2">
              {users.map((user) => (
                <UserAccessCard key={user.fingerprint} user={user} repoId={repoId} />
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-muted">No users configured</p>
          )}

          <GrantAccessForm repoId={repoId} />
        </div>

        {/* Branch Protection */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="flex items-center gap-2 mb-3">
            <Lock size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Branch Protection</h2>
          </div>

          {rules.length > 0 ? (
            <div className="space-y-2">
              {rules.map((rule) => (
                <BranchProtectionRule key={rule.branch} rule={rule} repoId={repoId} />
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-muted">No branch protection rules configured</p>
          )}

          <AddBranchProtectionForm repoId={repoId} />
        </div>
      </div>
    </div>
  );
}

export default AccessPage;
