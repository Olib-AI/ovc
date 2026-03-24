import { useState, useMemo } from 'react';
import {
  Plus,
  Trash2,
  Save,
  X,
  ChevronDown,
  ChevronRight,
  Pencil,
  Code,
  FileText,
  AlertTriangle,
} from 'lucide-react';
import {
  useActionsList,
  usePutActionConfig,
  useDeleteActionConfig,
} from '../hooks/useActions.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';
import type { ActionConfigDetail, ActionInfo } from '../api/types.ts';

interface ActionConfigEditorProps {
  repoId: string;
}

const TRIGGER_OPTIONS = [
  'manual',
  'pre-commit',
  'post-commit',
  'pre-push',
  'pre-merge',
  'post-merge',
  'on-fail',
  'schedule',
  'pull-request',
] as const;

const CATEGORY_OPTIONS = [
  'lint',
  'format',
  'build',
  'test',
  'quality',
  'security',
  'audit',
  'custom',
] as const;

const CATEGORY_COLORS: Record<string, string> = {
  lint: 'bg-blue-500/20 text-blue-300',
  format: 'bg-purple-500/20 text-purple-300',
  build: 'bg-orange-500/20 text-orange-300',
  test: 'bg-green-500/20 text-green-300',
  quality: 'bg-teal-500/20 text-teal-300',
  security: 'bg-red-500/20 text-red-300',
  audit: 'bg-red-500/20 text-red-300',
  custom: 'bg-gray-500/20 text-gray-300',
};

function getEmptyAction(): ActionConfigDetail {
  return {
    name: '',
    command: '',
    display_name: '',
    trigger: ['manual'],
    category: 'custom',
    timeout: 30,
    working_dir: '',
    condition_paths: [],
    depends_on: [],
    builtin: '',
    auto_fix: false,
    continue_on_error: false,
    env: {},
    docker: false,
  };
}

interface ActionFormProps {
  initial: ActionConfigDetail;
  isNew: boolean;
  onSave: (config: ActionConfigDetail) => void;
  onCancel: () => void;
  isSaving: boolean;
}

function ActionForm({ initial, isNew, onSave, onCancel, isSaving }: ActionFormProps) {
  const [form, setForm] = useState<ActionConfigDetail>(initial);
  const [showRawYaml, setShowRawYaml] = useState(false);
  const [conditionPathInput, setConditionPathInput] = useState('');
  const [dependsOnInput, setDependsOnInput] = useState('');
  const [envKeyInput, setEnvKeyInput] = useState('');
  const [envValueInput, setEnvValueInput] = useState('');

  const rawYaml = useMemo(() => {
    const lines: string[] = [];
    lines.push(`name: ${form.name}`);
    lines.push(`command: ${form.command}`);
    if (form.display_name) lines.push(`display_name: ${form.display_name}`);
    if (form.category) lines.push(`category: ${form.category}`);
    if (form.trigger && form.trigger.length > 0) {
      lines.push(`trigger:`);
      for (const t of form.trigger) {
        lines.push(`  - ${t}`);
      }
    }
    if (form.timeout !== undefined) lines.push(`timeout: ${form.timeout}`);
    if (form.working_dir) lines.push(`working_dir: ${form.working_dir}`);
    if (form.auto_fix) lines.push(`auto_fix: true`);
    if (form.continue_on_error) lines.push(`continue_on_error: true`);
    if (form.docker) lines.push(`docker: true`);
    if (form.builtin) lines.push(`builtin: ${form.builtin}`);
    if (form.condition_paths && form.condition_paths.length > 0) {
      lines.push(`condition_paths:`);
      for (const p of form.condition_paths) {
        lines.push(`  - ${p}`);
      }
    }
    if (form.depends_on && form.depends_on.length > 0) {
      lines.push(`depends_on:`);
      for (const d of form.depends_on) {
        lines.push(`  - ${d}`);
      }
    }
    if (form.env && Object.keys(form.env).length > 0) {
      lines.push(`env:`);
      for (const [k, v] of Object.entries(form.env)) {
        lines.push(`  ${k}: ${v}`);
      }
    }
    return lines.join('\n');
  }, [form]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    onSave(form);
  }

  function handleToggleTrigger(trigger: string) {
    setForm((prev) => {
      const triggers = prev.trigger ?? [];
      const next = triggers.includes(trigger)
        ? triggers.filter((t) => t !== trigger)
        : [...triggers, trigger];
      return { ...prev, trigger: next };
    });
  }

  function handleAddConditionPath() {
    const trimmed = conditionPathInput.trim();
    if (!trimmed) return;
    setForm((prev) => ({
      ...prev,
      condition_paths: [...(prev.condition_paths ?? []), trimmed],
    }));
    setConditionPathInput('');
  }

  function handleRemoveConditionPath(idx: number) {
    setForm((prev) => ({
      ...prev,
      condition_paths: (prev.condition_paths ?? []).filter((_, i) => i !== idx),
    }));
  }

  function handleAddDependsOn() {
    const trimmed = dependsOnInput.trim();
    if (!trimmed) return;
    setForm((prev) => ({
      ...prev,
      depends_on: [...(prev.depends_on ?? []), trimmed],
    }));
    setDependsOnInput('');
  }

  function handleRemoveDependsOn(idx: number) {
    setForm((prev) => ({
      ...prev,
      depends_on: (prev.depends_on ?? []).filter((_, i) => i !== idx),
    }));
  }

  function handleAddEnv() {
    const key = envKeyInput.trim();
    const value = envValueInput.trim();
    if (!key) return;
    setForm((prev) => ({
      ...prev,
      env: { ...(prev.env ?? {}), [key]: value },
    }));
    setEnvKeyInput('');
    setEnvValueInput('');
  }

  function handleRemoveEnv(key: string) {
    setForm((prev) => {
      const next = { ...(prev.env ?? {}) };
      delete next[key];
      return { ...prev, env: next };
    });
  }

  if (showRawYaml) {
    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-text-primary">
            {isNew ? 'New Action' : `Edit: ${form.display_name || form.name}`} (Raw YAML)
          </h3>
          <button
            onClick={() => setShowRawYaml(false)}
            className="flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-accent"
          >
            <FileText size={11} />
            Form View
          </button>
        </div>
        <textarea
          value={rawYaml}
          readOnly
          className="h-64 w-full resize-none rounded border border-border bg-navy-950 p-3 font-mono text-[11px] leading-relaxed text-text-secondary focus:border-accent focus:outline-none"
        />
        <p className="text-[10px] text-text-muted">
          Read-only preview. Edit using the form view for validated changes.
        </p>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="flex items-center gap-1 rounded border border-border px-3 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
          >
            <X size={12} />
            Cancel
          </button>
        </div>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-text-primary">
          {isNew ? 'Create New Action' : `Edit: ${form.display_name || form.name}`}
        </h3>
        <button
          type="button"
          onClick={() => setShowRawYaml(true)}
          className="flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-accent"
        >
          <Code size={11} />
          Raw YAML
        </button>
      </div>

      {/* Name */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">
          Name <span className="text-red-400">*</span>
        </label>
        <input
          type="text"
          value={form.name}
          onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))}
          disabled={!isNew}
          placeholder="e.g. eslint-check"
          className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none disabled:opacity-60"
        />
      </div>

      {/* Display Name */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Display Name</label>
        <input
          type="text"
          value={form.display_name ?? ''}
          onChange={(e) => setForm((prev) => ({ ...prev, display_name: e.target.value }))}
          placeholder="e.g. ESLint Check"
          className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
        />
      </div>

      {/* Command */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">
          Command <span className="text-red-400">*</span>
        </label>
        <input
          type="text"
          value={form.command}
          onChange={(e) => setForm((prev) => ({ ...prev, command: e.target.value }))}
          placeholder="e.g. npx eslint ."
          className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
        />
      </div>

      {/* Category */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Category</label>
        <div className="flex flex-wrap gap-1.5">
          {CATEGORY_OPTIONS.map((cat) => (
            <button
              key={cat}
              type="button"
              onClick={() => setForm((prev) => ({ ...prev, category: cat }))}
              className={`rounded-full border px-2.5 py-1 text-[10px] font-medium transition-colors ${
                form.category === cat
                  ? `${CATEGORY_COLORS[cat] ?? 'bg-gray-500/20 text-gray-300'} border-current`
                  : 'border-border text-text-muted hover:text-text-primary'
              }`}
            >
              {cat}
            </button>
          ))}
        </div>
      </div>

      {/* Triggers */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Triggers</label>
        <div className="flex flex-wrap gap-1.5">
          {TRIGGER_OPTIONS.map((trigger) => {
            const isSelected = (form.trigger ?? []).includes(trigger);
            return (
              <button
                key={trigger}
                type="button"
                onClick={() => handleToggleTrigger(trigger)}
                className={`rounded-full border px-2.5 py-1 text-[10px] font-medium transition-colors ${
                  isSelected
                    ? 'border-accent/40 bg-accent/15 text-accent'
                    : 'border-border text-text-muted hover:text-text-primary'
                }`}
              >
                {trigger}
              </button>
            );
          })}
        </div>
      </div>

      {/* Timeout + Working Dir */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="mb-1 block text-[11px] font-medium text-text-muted">Timeout (s)</label>
          <input
            type="number"
            value={form.timeout ?? 30}
            onChange={(e) => setForm((prev) => ({ ...prev, timeout: parseInt(e.target.value) || 30 }))}
            min={1}
            className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 text-xs text-text-secondary focus:border-accent focus:outline-none"
          />
        </div>
        <div>
          <label className="mb-1 block text-[11px] font-medium text-text-muted">Working Dir</label>
          <input
            type="text"
            value={form.working_dir ?? ''}
            onChange={(e) => setForm((prev) => ({ ...prev, working_dir: e.target.value }))}
            placeholder="."
            className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
        </div>
      </div>

      {/* Builtin */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Builtin</label>
        <input
          type="text"
          value={form.builtin ?? ''}
          onChange={(e) => setForm((prev) => ({ ...prev, builtin: e.target.value }))}
          placeholder="e.g. eslint, prettier, cargo-clippy"
          className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
        />
      </div>

      {/* Boolean flags */}
      <div className="flex gap-6">
        <label className="flex items-center gap-2 text-xs text-text-secondary">
          <input
            type="checkbox"
            checked={form.auto_fix ?? false}
            onChange={(e) => setForm((prev) => ({ ...prev, auto_fix: e.target.checked }))}
            className="rounded border-border"
          />
          Auto Fix
        </label>
        <label className="flex items-center gap-2 text-xs text-text-secondary">
          <input
            type="checkbox"
            checked={form.continue_on_error ?? false}
            onChange={(e) => setForm((prev) => ({ ...prev, continue_on_error: e.target.checked }))}
            className="rounded border-border"
          />
          Continue on Error
        </label>
        <label className="flex items-center gap-2 text-xs text-text-secondary">
          <input
            type="checkbox"
            checked={form.docker ?? false}
            onChange={(e) => setForm((prev) => ({ ...prev, docker: e.target.checked }))}
            className="rounded border-border"
          />
          Docker
        </label>
      </div>

      {/* Condition Paths */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Condition Paths</label>
        {(form.condition_paths ?? []).length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1">
            {(form.condition_paths ?? []).map((p, i) => (
              <span
                key={`${p}-${i}`}
                className="flex items-center gap-1 rounded bg-navy-800 px-2 py-0.5 font-mono text-[10px] text-text-secondary"
              >
                {p}
                <button
                  type="button"
                  onClick={() => handleRemoveConditionPath(i)}
                  className="text-red-400 hover:text-red-300"
                >
                  <X size={10} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="flex gap-2">
          <input
            type="text"
            value={conditionPathInput}
            onChange={(e) => setConditionPathInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleAddConditionPath(); } }}
            placeholder="e.g. src/**/*.ts"
            className="flex-1 rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          <button
            type="button"
            onClick={handleAddConditionPath}
            className="rounded border border-border px-2 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
          >
            <Plus size={12} />
          </button>
        </div>
      </div>

      {/* Depends On */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Depends On</label>
        {(form.depends_on ?? []).length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1">
            {(form.depends_on ?? []).map((d, i) => (
              <span
                key={`${d}-${i}`}
                className="flex items-center gap-1 rounded bg-navy-800 px-2 py-0.5 font-mono text-[10px] text-text-secondary"
              >
                {d}
                <button
                  type="button"
                  onClick={() => handleRemoveDependsOn(i)}
                  className="text-red-400 hover:text-red-300"
                >
                  <X size={10} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="flex gap-2">
          <input
            type="text"
            value={dependsOnInput}
            onChange={(e) => setDependsOnInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleAddDependsOn(); } }}
            placeholder="e.g. build-check"
            className="flex-1 rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          <button
            type="button"
            onClick={handleAddDependsOn}
            className="rounded border border-border px-2 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
          >
            <Plus size={12} />
          </button>
        </div>
      </div>

      {/* Environment Variables */}
      <div>
        <label className="mb-1 block text-[11px] font-medium text-text-muted">Environment Variables</label>
        {form.env && Object.keys(form.env).length > 0 && (
          <div className="mb-2 space-y-1">
            {Object.entries(form.env).map(([k, v]) => (
              <div
                key={k}
                className="flex items-center justify-between rounded bg-navy-800 px-2 py-1"
              >
                <span className="font-mono text-[10px] text-text-secondary">
                  {k}={v}
                </span>
                <button
                  type="button"
                  onClick={() => handleRemoveEnv(k)}
                  className="text-red-400 hover:text-red-300"
                >
                  <X size={10} />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="flex gap-2">
          <input
            type="text"
            value={envKeyInput}
            onChange={(e) => setEnvKeyInput(e.target.value.toUpperCase())}
            placeholder="KEY"
            className="w-1/3 rounded border border-border bg-navy-950 px-2 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          <input
            type="text"
            value={envValueInput}
            onChange={(e) => setEnvValueInput(e.target.value)}
            placeholder="value"
            className="flex-1 rounded border border-border bg-navy-950 px-2 py-1.5 font-mono text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          <button
            type="button"
            onClick={handleAddEnv}
            className="rounded border border-border px-2 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
          >
            <Plus size={12} />
          </button>
        </div>
      </div>

      {/* Actions */}
      <div className="flex justify-end gap-2 border-t border-border pt-4">
        <button
          type="button"
          onClick={onCancel}
          className="flex items-center gap-1 rounded border border-border px-3 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
        >
          <X size={12} />
          Cancel
        </button>
        <button
          type="submit"
          disabled={isSaving || !form.name.trim() || !form.command.trim()}
          className="flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
        >
          <Save size={12} />
          {isSaving ? 'Saving...' : isNew ? 'Create Action' : 'Save Changes'}
        </button>
      </div>
    </form>
  );
}

function ActionConfigEditor({ repoId }: ActionConfigEditorProps) {
  const { data, isLoading, error } = useActionsList(repoId);
  const putConfig = usePutActionConfig(repoId);
  const deleteConfig = useDeleteActionConfig(repoId);
  const toast = useToast();
  const [editingAction, setEditingAction] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [expandedActions, setExpandedActions] = useState<Set<string>>(new Set());
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading actions..." />;
  }

  if (error) {
    return (
      <div className="flex items-center gap-2 p-6 text-sm text-red-400">
        <AlertTriangle size={16} />
        Failed to load actions: {error instanceof Error ? error.message : 'Unknown error'}
      </div>
    );
  }

  const actions: ActionInfo[] = data?.actions ?? [];

  function toggleExpand(name: string) {
    setExpandedActions((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  function handleSave(config: ActionConfigDetail) {
    putConfig.mutate(
      { name: config.name, config },
      {
        onSuccess: () => {
          toast.success(`Action "${config.name}" saved`);
          setEditingAction(null);
          setIsCreating(false);
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : 'Failed to save action');
        },
      },
    );
  }

  function handleDelete(name: string) {
    deleteConfig.mutate(name, {
      onSuccess: () => {
        toast.success(`Action "${name}" deleted`);
        setDeleteConfirm(null);
        setEditingAction(null);
      },
      onError: (err) => {
        toast.error(err instanceof Error ? err.message : 'Failed to delete action');
      },
    });
  }

  function actionToConfigDetail(action: ActionInfo): ActionConfigDetail {
    return {
      name: action.name,
      command: '',
      display_name: action.display_name,
      trigger: action.triggers,
      category: action.category,
      timeout: 30,
      working_dir: '',
      condition_paths: [],
      depends_on: [],
      builtin: action.tool ?? '',
      auto_fix: false,
      continue_on_error: false,
      env: {},
      docker: false,
    };
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl p-6">
        {/* Header */}
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-text-primary">Actions Configuration</h2>
            <p className="mt-0.5 text-[11px] text-text-muted">
              {actions.length} action{actions.length !== 1 ? 's' : ''} configured
            </p>
          </div>
          {!isCreating && (
            <button
              onClick={() => {
                setIsCreating(true);
                setEditingAction(null);
              }}
              className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light"
            >
              <Plus size={13} />
              New Action
            </button>
          )}
        </div>

        {/* Create form */}
        {isCreating && (
          <div className="mb-4 rounded-lg border border-accent/30 bg-navy-900 p-4">
            <ActionForm
              initial={getEmptyAction()}
              isNew={true}
              onSave={handleSave}
              onCancel={() => setIsCreating(false)}
              isSaving={putConfig.isPending}
            />
          </div>
        )}

        {/* Actions list */}
        {actions.length === 0 && !isCreating ? (
          <div className="rounded-lg border border-border bg-navy-900 p-8 text-center">
            <p className="text-sm text-text-muted">No actions configured yet.</p>
            <button
              onClick={() => setIsCreating(true)}
              className="mt-3 flex items-center gap-1.5 mx-auto rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light"
            >
              <Plus size={13} />
              Create First Action
            </button>
          </div>
        ) : (
          <div className="space-y-2">
            {actions.map((action) => {
              const isExpanded = expandedActions.has(action.name);
              const isEditing = editingAction === action.name;
              const catColor = CATEGORY_COLORS[action.category] ?? 'bg-gray-500/20 text-gray-300';

              return (
                <div
                  key={action.name}
                  className="rounded-lg border border-border bg-navy-900 overflow-hidden"
                >
                  {/* Action row */}
                  <button
                    onClick={() => {
                      if (isEditing) return;
                      toggleExpand(action.name);
                    }}
                    className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-surface-hover"
                  >
                    {isExpanded ? (
                      <ChevronDown size={14} className="flex-shrink-0 text-text-muted" />
                    ) : (
                      <ChevronRight size={14} className="flex-shrink-0 text-text-muted" />
                    )}

                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-text-primary">
                          {action.display_name}
                        </span>
                        <span className="font-mono text-[10px] text-text-muted">
                          {action.name}
                        </span>
                      </div>
                      <div className="mt-1 flex items-center gap-1.5">
                        <span className={`rounded-full px-2 py-0.5 text-[9px] font-medium ${catColor}`}>
                          {action.category}
                        </span>
                        {action.triggers.map((t) => (
                          <span
                            key={t}
                            className="rounded-full bg-navy-800 px-1.5 py-0.5 text-[9px] text-text-muted"
                          >
                            {t}
                          </span>
                        ))}
                        {action.last_run && (
                          <span
                            className={`text-[10px] font-medium ${
                              action.last_run.status === 'passed'
                                ? 'text-green-400'
                                : action.last_run.status === 'failed'
                                  ? 'text-red-400'
                                  : 'text-gray-400'
                            }`}
                          >
                            {action.last_run.status}
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <button
                        onClick={() => {
                          setEditingAction(action.name);
                          setExpandedActions((prev) => new Set(prev).add(action.name));
                          setIsCreating(false);
                        }}
                        className="rounded p-1.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-accent"
                        title="Edit action"
                      >
                        <Pencil size={13} />
                      </button>
                      <button
                        onClick={() => setDeleteConfirm(action.name)}
                        className="rounded p-1.5 text-text-muted transition-colors hover:bg-red-500/10 hover:text-red-400"
                        title="Delete action"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  </button>

                  {/* Expanded content */}
                  {isExpanded && (
                    <div className="border-t border-border px-4 py-3">
                      {isEditing ? (
                        <ActionForm
                          initial={actionToConfigDetail(action)}
                          isNew={false}
                          onSave={handleSave}
                          onCancel={() => setEditingAction(null)}
                          isSaving={putConfig.isPending}
                        />
                      ) : (
                        <div className="space-y-2 text-xs text-text-secondary">
                          <div className="grid grid-cols-2 gap-2">
                            <div>
                              <span className="text-[10px] text-text-muted">Category</span>
                              <p>{action.category}</p>
                            </div>
                            <div>
                              <span className="text-[10px] text-text-muted">Language</span>
                              <p>{action.language ?? 'N/A'}</p>
                            </div>
                            <div>
                              <span className="text-[10px] text-text-muted">Tool</span>
                              <p>{action.tool ?? 'N/A'}</p>
                            </div>
                            <div>
                              <span className="text-[10px] text-text-muted">Triggers</span>
                              <p>{action.triggers.join(', ')}</p>
                            </div>
                          </div>
                          {action.last_run && (
                            <div className="rounded bg-navy-800 p-2">
                              <span className="text-[10px] text-text-muted">Last Run</span>
                              <p>
                                Status:{' '}
                                <span
                                  className={
                                    action.last_run.status === 'passed'
                                      ? 'text-green-400'
                                      : 'text-red-400'
                                  }
                                >
                                  {action.last_run.status}
                                </span>
                                {' | '}
                                Duration: {action.last_run.duration_ms}ms
                              </p>
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* Delete confirmation modal */}
        {deleteConfirm && (
          <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div className="fixed inset-0 bg-navy-950/70" onClick={() => setDeleteConfirm(null)} />
            <div className="relative z-10 w-full max-w-sm rounded-xl border border-border bg-navy-800 p-5 shadow-2xl">
              <div className="mb-3 flex items-center gap-2">
                <AlertTriangle size={18} className="text-red-400" />
                <h3 className="text-sm font-semibold text-text-primary">Delete Action</h3>
              </div>
              <p className="mb-4 text-xs text-text-secondary">
                Are you sure you want to delete{' '}
                <span className="font-mono font-medium text-text-primary">{deleteConfirm}</span>?
                This cannot be undone.
              </p>
              <div className="flex justify-end gap-2">
                <button
                  onClick={() => setDeleteConfirm(null)}
                  className="rounded border border-border px-3 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
                >
                  Cancel
                </button>
                <button
                  onClick={() => handleDelete(deleteConfirm)}
                  disabled={deleteConfig.isPending}
                  className="flex items-center gap-1 rounded bg-red-500 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-400 disabled:opacity-50"
                >
                  <Trash2 size={12} />
                  {deleteConfig.isPending ? 'Deleting...' : 'Delete'}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default ActionConfigEditor;
