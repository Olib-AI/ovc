import { useState } from 'react';
import { Download, X, Archive, GitCommitVertical, RefreshCw } from 'lucide-react';
import { downloadArchiveWithRef } from '../api/client.ts';
import { useToast } from '../contexts/ToastContext.tsx';

interface ArchiveDialogProps {
  repoId: string;
  repoName: string;
  /** Pre-fills the ref input — useful when opening from a commit action bar. */
  defaultRef?: string;
  onClose: () => void;
}

function ArchiveDialog({ repoId, repoName, defaultRef = '', onClose }: ArchiveDialogProps) {
  const toast = useToast();
  const [format, setFormat] = useState<'tar' | 'zip'>('tar');
  const [commitRef, setCommitRef] = useState(defaultRef);
  const [isDownloading, setIsDownloading] = useState(false);

  function handleDownload() {
    setIsDownloading(true);
    const ref = commitRef.trim() || undefined;
    const ext = format === 'zip' ? 'zip' : 'tar.gz';
    const filename = ref
      ? `${repoName}-${ref.slice(0, 12)}.${ext}`
      : `${repoName}.${ext}`;

    downloadArchiveWithRef(repoId, format, ref)
      .then((blob) => {
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        toast.success(`Archive "${filename}" downloaded`);
        onClose();
      })
      .catch((err: Error) => {
        toast.error(err.message);
      })
      .finally(() => setIsDownloading(false));
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ backgroundColor: 'var(--theme-modal-backdrop)' }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="w-96 rounded-lg border border-border bg-navy-800 shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Archive size={16} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">Export Archive</h2>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
          >
            <X size={14} />
          </button>
        </div>

        {/* Body */}
        <div className="space-y-4 p-4">
          {/* Format selector */}
          <div>
            <label className="mb-2 block text-xs font-medium text-text-muted">Format</label>
            <div className="grid grid-cols-2 gap-2">
              {(['tar', 'zip'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setFormat(f)}
                  className={`rounded border px-3 py-2 text-left text-xs transition-colors ${
                    format === f
                      ? 'border-accent bg-accent/10 text-accent'
                      : 'border-border bg-navy-950 text-text-secondary hover:border-accent/40 hover:text-text-primary'
                  }`}
                >
                  <p className="font-semibold">{f === 'tar' ? 'TAR (tar.gz)' : 'ZIP'}</p>
                  <p className="mt-0.5 text-[11px] text-text-muted">
                    {f === 'tar' ? 'Gzip-compressed tarball' : 'Standard ZIP archive'}
                  </p>
                </button>
              ))}
            </div>
          </div>

          {/* Commit / ref input */}
          <div>
            <label
              htmlFor="archive-ref"
              className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-text-muted"
            >
              <GitCommitVertical size={12} />
              Commit / Branch / Tag
              <span className="text-[11px] text-text-muted/60">(optional — defaults to HEAD)</span>
            </label>
            <input
              id="archive-ref"
              value={commitRef}
              onChange={(e) => setCommitRef(e.target.value)}
              placeholder="HEAD, main, v1.0.0, abc1234..."
              className="w-full rounded border border-border bg-navy-950 px-3 py-1.5 font-mono text-xs text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
          </div>

          {/* Output name preview */}
          <div className="rounded border border-border/50 bg-navy-950 px-3 py-2">
            <p className="mb-0.5 text-[10px] font-medium uppercase tracking-wide text-text-muted">
              Output filename
            </p>
            <p className="truncate font-mono text-xs text-text-secondary">
              {commitRef.trim()
                ? `${repoName}-${commitRef.trim().slice(0, 12)}.${format === 'zip' ? 'zip' : 'tar.gz'}`
                : `${repoName}.${format === 'zip' ? 'zip' : 'tar.gz'}`}
            </p>
          </div>
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-2 border-t border-border px-4 py-3">
          <button
            onClick={onClose}
            className="rounded px-3 py-1.5 text-xs text-text-muted transition-colors hover:text-text-primary"
          >
            Cancel
          </button>
          <button
            onClick={handleDownload}
            disabled={isDownloading}
            className="flex items-center gap-1.5 rounded bg-accent px-3 py-1.5 text-xs font-medium text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {isDownloading ? (
              <RefreshCw size={12} className="animate-spin" />
            ) : (
              <Download size={12} />
            )}
            {isDownloading ? 'Downloading...' : 'Download'}
          </button>
        </div>
      </div>
    </div>
  );
}

export default ArchiveDialog;
