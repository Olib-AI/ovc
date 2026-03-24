import { useState } from 'react';
import { RefreshCw, Zap, Eye, Languages, Save, X, KeyRound, Plus, Trash2, Container, AlertTriangle } from 'lucide-react';
import {
  useActionsConfig,
  usePutActionsConfig,
  useDetectLanguages,
  useInitActions,
  useActionSecrets,
  usePutActionSecret,
  useDeleteActionSecret,
  useDockerStatus,
} from '../hooks/useActions.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';

interface ActionsSettingsProps {
  repoId: string;
}

const LANGUAGE_BADGE_COLORS: Record<string, string> = {
  rust: 'bg-orange-500/20 text-orange-300 border-orange-500/30',
  javascript: 'bg-yellow-500/20 text-yellow-300 border-yellow-500/30',
  typescript: 'bg-blue-500/20 text-blue-300 border-blue-500/30',
  python: 'bg-blue-600/20 text-blue-300 border-blue-600/30',
  go: 'bg-cyan-500/20 text-cyan-300 border-cyan-500/30',
  ruby: 'bg-red-500/20 text-red-300 border-red-500/30',
  java: 'bg-red-600/20 text-red-400 border-red-600/30',
  c: 'bg-gray-500/20 text-gray-300 border-gray-500/30',
  cpp: 'bg-purple-500/20 text-purple-300 border-purple-500/30',
  shell: 'bg-green-600/20 text-green-300 border-green-600/30',
  elixir: 'bg-purple-600/20 text-purple-300 border-purple-600/30',
  csharp: 'bg-green-500/20 text-green-300 border-green-500/30',
  'c#': 'bg-green-500/20 text-green-300 border-green-500/30',
  swift: 'bg-orange-600/20 text-orange-300 border-orange-600/30',
  php: 'bg-indigo-500/20 text-indigo-300 border-indigo-500/30',
  kotlin: 'bg-violet-500/20 text-violet-300 border-violet-500/30',
  dart: 'bg-sky-500/20 text-sky-300 border-sky-500/30',
  deno: 'bg-lime-500/20 text-lime-300 border-lime-500/30',
};

function ActionsSettings({ repoId }: ActionsSettingsProps) {
  const { data: configData, isLoading: configLoading } = useActionsConfig(repoId);
  const putConfig = usePutActionsConfig(repoId);
  const {
    data: detection,
    isLoading: detectLoading,
    refetch: refetchDetection,
  } = useDetectLanguages(repoId);
  const initMutation = useInitActions(repoId);
  const { data: secretsData, isLoading: secretsLoading } = useActionSecrets(repoId);
  const putSecret = usePutActionSecret(repoId);
  const deleteSecret = useDeleteActionSecret(repoId);
  const { data: dockerStatus, isLoading: dockerLoading } = useDockerStatus(repoId);
  const toast = useToast();
  const [isDetecting, setIsDetecting] = useState(false);

  // Secret form state
  const [newSecretName, setNewSecretName] = useState('');
  const [newSecretValue, setNewSecretValue] = useState('');

  const configText = configData?.content ?? '';
  const [prevConfigText, setPrevConfigText] = useState(configText);
  const [editedConfig, setEditedConfig] = useState(configText);
  const [isDirty, setIsDirty] = useState(false);

  if (configText !== prevConfigText) {
    setPrevConfigText(configText);
    setEditedConfig(configText);
    setIsDirty(false);
  }

  function handleConfigChange(value: string) {
    setEditedConfig(value);
    setIsDirty(value !== configText);
  }

  function handleSaveConfig() {
    putConfig.mutate(editedConfig, {
      onSuccess: () => {
        toast.success('Configuration saved');
        setIsDirty(false);
      },
      onError: (err) => {
        toast.error(err instanceof Error ? err.message : 'Failed to save configuration');
      },
    });
  }

  function handleCancelEdit() {
    setEditedConfig(configText);
    setIsDirty(false);
  }

  function handleRedetect() {
    setIsDetecting(true);
    void refetchDetection().finally(() => setIsDetecting(false));
  }

  function handleAddSecret() {
    const trimmedName = newSecretName.trim();
    const trimmedValue = newSecretValue.trim();
    if (!trimmedName || !trimmedValue) return;
    putSecret.mutate(
      { name: trimmedName, value: trimmedValue },
      {
        onSuccess: () => {
          toast.success(`Secret "${trimmedName}" saved`);
          setNewSecretName('');
          setNewSecretValue('');
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : 'Failed to save secret');
        },
      },
    );
  }

  function handleDeleteSecret(name: string) {
    deleteSecret.mutate(name, {
      onSuccess: () => {
        toast.success(`Secret "${name}" deleted`);
      },
      onError: (err) => {
        toast.error(err instanceof Error ? err.message : 'Failed to delete secret');
      },
    });
  }

  if (configLoading || detectLoading) {
    return <LoadingSpinner className="h-full" message="Loading settings..." />;
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-xl space-y-6 p-6">
        {/* Docker Status */}
        {!dockerLoading && dockerStatus && (
          <div className="rounded-lg border border-border bg-navy-900 p-4">
            <div className="mb-3 flex items-center gap-2">
              <Container size={16} className="text-accent" />
              <h2 className="text-sm font-semibold text-text-primary">Docker</h2>
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2">
                <span className="text-[11px] text-text-muted">Status</span>
                <span
                  className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                    dockerStatus.enabled && dockerStatus.available
                      ? 'bg-green-500/20 text-green-300'
                      : dockerStatus.enabled && !dockerStatus.available
                        ? 'bg-red-500/20 text-red-300'
                        : 'bg-gray-500/20 text-gray-300'
                  }`}
                >
                  {dockerStatus.enabled
                    ? dockerStatus.available
                      ? 'Enabled & Available'
                      : 'Enabled but Unavailable'
                    : 'Disabled'}
                </span>
              </div>

              {dockerStatus.version && (
                <div className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2">
                  <span className="text-[11px] text-text-muted">Version</span>
                  <span className="font-mono text-[11px] text-text-secondary">{dockerStatus.version}</span>
                </div>
              )}

              <div className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2">
                <span className="text-[11px] text-text-muted">Image</span>
                <span className="font-mono text-[11px] text-text-secondary">{dockerStatus.image}</span>
              </div>

              <div className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2">
                <span className="text-[11px] text-text-muted">Pull Policy</span>
                <span className="text-[11px] text-text-secondary">{dockerStatus.pull_policy}</span>
              </div>

              {dockerStatus.enabled && !dockerStatus.available && dockerStatus.reason && (
                <div className="flex items-center gap-2 rounded border border-yellow-500/30 bg-yellow-500/5 px-3 py-2">
                  <AlertTriangle size={13} className="flex-shrink-0 text-yellow-400" />
                  <span className="text-[11px] text-yellow-300">{dockerStatus.reason}</span>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Detected Languages */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Languages size={16} className="text-accent" />
              <h2 className="text-sm font-semibold text-text-primary">Detected Languages</h2>
            </div>
            <button
              onClick={handleRedetect}
              disabled={isDetecting}
              className="flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-text-secondary transition-colors hover:border-accent/40 hover:text-accent disabled:opacity-50"
            >
              <RefreshCw size={11} className={isDetecting ? 'animate-spin' : ''} />
              Re-detect
            </button>
          </div>

          {detection && detection.languages.length > 0 ? (
            <div className="space-y-2">
              {detection.languages.map((lang) => {
                const langKey = lang.language.toLowerCase();
                const badgeColor =
                  langKey in LANGUAGE_BADGE_COLORS
                    ? LANGUAGE_BADGE_COLORS[langKey]
                    : 'bg-gray-500/20 text-gray-300 border-gray-500/30';
                return (
                  <div
                    key={`${lang.language}-${lang.marker_file}`}
                    className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2"
                  >
                    <div className="flex items-center gap-2">
                      <span className={`rounded border px-2 py-0.5 text-[11px] font-medium ${badgeColor}`}>
                        {lang.language}
                      </span>
                      <span className="font-mono text-[10px] text-text-muted">{lang.marker_file}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-[10px] text-text-muted">{lang.root_dir}</span>
                      <span
                        className={`rounded px-1.5 py-0.5 text-[9px] font-medium ${
                          lang.confidence === 'high'
                            ? 'bg-green-500/20 text-green-300'
                            : lang.confidence === 'medium'
                              ? 'bg-yellow-500/20 text-yellow-300'
                              : 'bg-gray-500/20 text-gray-300'
                        }`}
                      >
                        {lang.confidence}
                      </span>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="text-xs text-text-muted">No languages detected</p>
          )}
        </div>

        {/* Current Config */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center gap-2">
            <Eye size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Current Configuration</h2>
          </div>
          <textarea
            value={editedConfig}
            placeholder="No configuration found. Click 'Initialize Actions' to auto-detect and generate a config."
            onChange={(e) => handleConfigChange(e.target.value)}
            className="h-64 w-full resize-none rounded border border-border bg-navy-950 p-3 font-mono text-[11px] leading-relaxed text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          <div className="mt-2 flex items-center justify-between">
            <p className="text-[10px] text-text-muted">
              Edit the YAML configuration and click Save.
            </p>
            {isDirty && (
              <div className="flex gap-2">
                <button
                  onClick={handleCancelEdit}
                  className="flex items-center gap-1 rounded border border-border px-2.5 py-1 text-[11px] text-text-muted transition-colors hover:text-text-primary"
                >
                  <X size={11} />
                  Cancel
                </button>
                <button
                  onClick={handleSaveConfig}
                  disabled={putConfig.isPending}
                  className="flex items-center gap-1 rounded bg-accent px-2.5 py-1 text-[11px] font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
                >
                  <Save size={11} />
                  {putConfig.isPending ? 'Saving...' : 'Save Config'}
                </button>
              </div>
            )}
          </div>
        </div>

        {/* Secrets */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center gap-2">
            <KeyRound size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Secrets</h2>
          </div>

          {secretsLoading ? (
            <LoadingSpinner size={14} className="py-2" />
          ) : (
            <>
              {secretsData && secretsData.names.length > 0 ? (
                <div className="mb-3 space-y-1.5">
                  {secretsData.names.map((name) => (
                    <div
                      key={name}
                      className="flex items-center justify-between rounded border border-border bg-navy-800 px-3 py-2"
                    >
                      <div className="flex items-center gap-2">
                        <KeyRound size={11} className="text-text-muted" />
                        <span className="font-mono text-[11px] text-text-primary">{name}</span>
                      </div>
                      <button
                        onClick={() => handleDeleteSecret(name)}
                        disabled={deleteSecret.isPending}
                        className="flex items-center gap-1 rounded p-1 text-[10px] text-red-400 transition-colors hover:bg-red-400/10 disabled:opacity-50"
                        title={`Delete secret ${name}`}
                      >
                        <Trash2 size={11} />
                      </button>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="mb-3 text-xs text-text-muted">No secrets configured</p>
              )}

              <div className="rounded border border-border bg-navy-800 p-3">
                <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
                  Add Secret
                </div>
                <div className="flex gap-2">
                  <input
                    type="text"
                    placeholder="SECRET_NAME"
                    value={newSecretName}
                    onChange={(e) => setNewSecretName(e.target.value.toUpperCase())}
                    className="w-1/3 rounded border border-border bg-navy-950 px-2 py-1.5 font-mono text-[11px] text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
                  />
                  <input
                    type="password"
                    placeholder="value"
                    value={newSecretValue}
                    onChange={(e) => setNewSecretValue(e.target.value)}
                    className="flex-1 rounded border border-border bg-navy-950 px-2 py-1.5 font-mono text-[11px] text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
                  />
                  <button
                    onClick={handleAddSecret}
                    disabled={putSecret.isPending || !newSecretName.trim() || !newSecretValue.trim()}
                    className="flex items-center gap-1 rounded bg-accent px-2.5 py-1.5 text-[11px] font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
                  >
                    <Plus size={11} />
                    {putSecret.isPending ? 'Saving...' : 'Add'}
                  </button>
                </div>
              </div>
            </>
          )}
        </div>

        {/* Initialize / Reset */}
        <div className="rounded-lg border border-border bg-navy-900 p-4">
          <div className="mb-3 flex items-center gap-2">
            <Zap size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Initialize / Reset Config</h2>
          </div>
          <p className="mb-3 text-xs text-text-muted">
            Re-detect languages and generate a fresh configuration file based on your project.
          </p>
          <div className="flex gap-2">
            <button
              onClick={() => initMutation.mutate(false)}
              disabled={initMutation.isPending}
              className="flex items-center gap-1.5 rounded border border-border bg-surface px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent/40 hover:text-text-primary disabled:opacity-50"
            >
              {initMutation.isPending ? (
                <RefreshCw size={13} className="animate-spin" />
              ) : (
                <Zap size={13} />
              )}
              Initialize
            </button>
            <button
              onClick={() => initMutation.mutate(true)}
              disabled={initMutation.isPending}
              className="flex items-center gap-1.5 rounded border border-red-500/30 px-3 py-1.5 text-xs text-red-400 transition-colors hover:bg-red-500/10 disabled:opacity-50"
            >
              <RefreshCw size={13} />
              Force Reset
            </button>
          </div>
          {initMutation.isSuccess && (
            <p className="mt-2 text-xs text-green-400">Configuration initialized successfully.</p>
          )}
          {initMutation.isError && (
            <p className="mt-2 text-xs text-red-400">
              Failed: {initMutation.error instanceof Error ? initMutation.error.message : 'Unknown error'}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

export default ActionsSettings;
