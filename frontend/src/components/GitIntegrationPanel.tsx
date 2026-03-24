import { useState } from 'react';
import { GitMerge, Upload, Download, AlertTriangle, RefreshCw, CheckCircle, FolderOpen } from 'lucide-react';
import { gitImport, gitExport } from '../api/client.ts';
import { useToast } from '../contexts/ToastContext.tsx';

interface GitIntegrationPanelProps {
  repoId: string;
}

type OperationStatus = 'idle' | 'running' | 'success' | 'error';

interface OperationState {
  status: OperationStatus;
  message: string;
}

const IDLE: OperationState = { status: 'idle', message: '' };

function GitIntegrationPanel({ repoId }: GitIntegrationPanelProps) {
  const toast = useToast();
  const [importPath, setImportPath] = useState('');
  const [exportPath, setExportPath] = useState('');
  const [importOp, setImportOp] = useState<OperationState>(IDLE);
  const [exportOp, setExportOp] = useState<OperationState>(IDLE);

  function handleImport() {
    const path = importPath.trim();
    if (!path) return;
    setImportOp({ status: 'running', message: 'Importing from git repo...' });
    gitImport(repoId, path)
      .then((res) => {
        const msg = res.message || `Imported ${res.commits_imported} commit(s)`;
        setImportOp({ status: 'success', message: msg });
        toast.success(msg);
      })
      .catch((err: Error) => {
        setImportOp({ status: 'error', message: err.message });
        toast.error(err.message);
      });
  }

  function handleExport() {
    const path = exportPath.trim();
    if (!path) return;
    setExportOp({ status: 'running', message: 'Exporting to git repo...' });
    gitExport(repoId, path)
      .then((res) => {
        const msg = res.message || `Exported ${res.commits_exported} commit(s)`;
        setExportOp({ status: 'success', message: msg });
        toast.success(msg);
      })
      .catch((err: Error) => {
        setExportOp({ status: 'error', message: err.message });
        toast.error(err.message);
      });
  }

  return (
    <div className="rounded-lg border border-border bg-navy-900 p-4">
      <div className="flex items-center gap-2 mb-4">
        <GitMerge size={16} className="text-accent" />
        <h2 className="text-sm font-semibold text-text-primary">Git Integration</h2>
      </div>

      {/* Import from Git */}
      <div className="mb-4 rounded border border-border bg-navy-950 p-3">
        <div className="mb-2 flex items-center gap-2">
          <Download size={13} className="text-accent/80" />
          <span className="text-xs font-semibold text-text-primary">Import from Git Repo</span>
        </div>
        <p className="mb-3 text-[11px] text-text-muted leading-relaxed">
          Import commits and history from a local Git repository into this OVC repo.
        </p>

        {/* Warning */}
        <div className="mb-3 flex items-start gap-2 rounded border border-status-modified/30 bg-status-modified/5 px-2.5 py-2">
          <AlertTriangle size={12} className="mt-0.5 flex-shrink-0 text-status-modified" />
          <p className="text-[11px] text-text-secondary leading-relaxed">
            Importing will merge the git history into this repository. Existing commits are preserved.
          </p>
        </div>

        <div className="flex gap-2">
          <div className="relative flex-1">
            <FolderOpen size={12} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-text-muted" />
            <input
              value={importPath}
              onChange={(e) => {
                setImportPath(e.target.value);
                if (importOp.status !== 'idle') setImportOp(IDLE);
              }}
              placeholder="/path/to/git/repo"
              className="w-full rounded border border-border bg-navy-900 py-1.5 pl-7 pr-2 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          </div>
          <button
            onClick={handleImport}
            disabled={!importPath.trim() || importOp.status === 'running'}
            className="flex shrink-0 items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {importOp.status === 'running' ? (
              <RefreshCw size={11} className="animate-spin" />
            ) : (
              <Download size={11} />
            )}
            {importOp.status === 'running' ? 'Importing...' : 'Import'}
          </button>
        </div>

        <OperationResult op={importOp} />
      </div>

      {/* Export to Git */}
      <div className="rounded border border-border bg-navy-950 p-3">
        <div className="mb-2 flex items-center gap-2">
          <Upload size={13} className="text-accent/80" />
          <span className="text-xs font-semibold text-text-primary">Export to Git Repo</span>
        </div>
        <p className="mb-3 text-[11px] text-text-muted leading-relaxed">
          Export this OVC repository's commits and history to a local Git repository.
        </p>

        {/* Warning */}
        <div className="mb-3 flex items-start gap-2 rounded border border-status-deleted/30 bg-status-deleted/5 px-2.5 py-2">
          <AlertTriangle size={12} className="mt-0.5 flex-shrink-0 text-status-deleted" />
          <p className="text-[11px] text-text-secondary leading-relaxed">
            Exporting may overwrite existing data in the destination repository. Ensure you have a backup.
          </p>
        </div>

        <div className="flex gap-2">
          <div className="relative flex-1">
            <FolderOpen size={12} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-text-muted" />
            <input
              value={exportPath}
              onChange={(e) => {
                setExportPath(e.target.value);
                if (exportOp.status !== 'idle') setExportOp(IDLE);
              }}
              placeholder="/path/to/destination"
              className="w-full rounded border border-border bg-navy-900 py-1.5 pl-7 pr-2 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          </div>
          <button
            onClick={handleExport}
            disabled={!exportPath.trim() || exportOp.status === 'running'}
            className="flex shrink-0 items-center gap-1.5 rounded border border-status-deleted/40 bg-status-deleted/10 px-3 py-1.5 text-xs font-medium text-status-deleted transition-colors hover:bg-status-deleted/20 disabled:opacity-50"
          >
            {exportOp.status === 'running' ? (
              <RefreshCw size={11} className="animate-spin" />
            ) : (
              <Upload size={11} />
            )}
            {exportOp.status === 'running' ? 'Exporting...' : 'Export'}
          </button>
        </div>

        <OperationResult op={exportOp} />
      </div>
    </div>
  );
}

function OperationResult({ op }: { op: OperationState }) {
  if (op.status === 'idle' || op.status === 'running') return null;

  if (op.status === 'success') {
    return (
      <div className="mt-2 flex items-start gap-1.5 rounded border border-status-added/20 bg-status-added/5 px-2.5 py-1.5">
        <CheckCircle size={12} className="mt-0.5 flex-shrink-0 text-status-added" />
        <p className="text-[11px] text-text-secondary">{op.message}</p>
      </div>
    );
  }

  return (
    <div className="mt-2 flex items-start gap-1.5 rounded border border-status-deleted/20 bg-status-deleted/5 px-2.5 py-1.5">
      <AlertTriangle size={12} className="mt-0.5 flex-shrink-0 text-status-deleted" />
      <p className="text-[11px] text-status-deleted">{op.message}</p>
    </div>
  );
}

export default GitIntegrationPanel;
