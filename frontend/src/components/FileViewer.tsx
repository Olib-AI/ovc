import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import { useFileContent, usePutBlob, useDeleteBlob } from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';
import FileHistory from './FileHistory.tsx';
import { FileWarning, Eye, Code, ZoomIn, ZoomOut, Maximize2, Image as ImageIcon, History, Pencil, Save, X, Trash2, Info, AlertTriangle, Download } from 'lucide-react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { getExtension, detectLanguage, highlightLines } from '../utils/highlight.ts';

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.svg', '.webp', '.ico', '.bmp']);
const MARKDOWN_EXTENSIONS = new Set(['.md', '.markdown']);

/** Syntax highlighting is skipped for files above this threshold. */
const HIGHLIGHT_SIZE_LIMIT = 512_000;
/** Files above this threshold receive a stronger warning. */
const LARGE_FILE_SIZE_LIMIT = 2_000_000;
/** Maximum lines rendered before requiring explicit opt-in. */
const LINE_CAP = 10_000;

// Configure marked for GFM support
marked.setOptions({
  gfm: true,
  breaks: false,
});

interface FileViewerProps {
  repoId: string;
  filePath: string | null;
  browseRef?: string;
  highlightLine?: number;
  /** When true the file-history panel is shown immediately on mount */
  initialShowHistory?: boolean;
  /** Called after a file is deleted so the parent can clear selection */
  onFileDeleted?: () => void;
  /** Called when a breadcrumb directory segment is clicked */
  onNavigateToDir?: (path: string) => void;
}

function FileViewer({ repoId, filePath, browseRef, highlightLine, initialShowHistory = false, onFileDeleted, onNavigateToDir }: FileViewerProps) {
  const { data, isLoading, error } = useFileContent(repoId, filePath, browseRef);
  const highlightRef = useRef<HTMLTableRowElement>(null);
  const [viewMode, setViewMode] = useState<'preview' | 'raw'>('preview');
  const [showHistory, setShowHistory] = useState(initialShowHistory);
  const [imageZoom, setImageZoom] = useState(100);
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(null);

  // Editing state
  const [isEditing, setIsEditing] = useState(false);
  const [editContent, setEditContent] = useState('');
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  /** Current maximum number of lines to render. Starts at LINE_CAP, increments by LINE_CAP. */
  const [visibleLineCap, setVisibleLineCap] = useState(LINE_CAP);

  const toast = useToast();
  const putBlob = usePutBlob(repoId);
  const deleteBlob = useDeleteBlob(repoId);

  const handleStartEdit = useCallback(() => {
    if (data && !data.is_binary) {
      setEditContent(data.content);
      setIsEditing(true);
    }
  }, [data]);

  const handleCancelEdit = useCallback(() => {
    setIsEditing(false);
    setEditContent('');
  }, []);

  const handleSaveEdit = useCallback(() => {
    if (!filePath) return;
    putBlob.mutate(
      { path: filePath, content: editContent },
      {
        onSuccess: () => {
          toast.success(`Saved ${filePath}`);
          setIsEditing(false);
          setEditContent('');
        },
        onError: (err: Error) => {
          toast.error(`Failed to save: ${err.message}`);
        },
      },
    );
  }, [filePath, editContent, putBlob, toast]);

  const handleDelete = useCallback(() => {
    if (!filePath) return;
    deleteBlob.mutate(filePath, {
      onSuccess: () => {
        toast.success(`Deleted ${filePath}`);
        setShowDeleteConfirm(false);
        onFileDeleted?.();
      },
      onError: (err: Error) => {
        toast.error(`Failed to delete: ${err.message}`);
        setShowDeleteConfirm(false);
      },
    });
  }, [filePath, deleteBlob, toast, onFileDeleted]);

  useEffect(() => {
    if (data && highlightLine && highlightRef.current) {
      highlightRef.current.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [highlightLine, data]);

  const ext = filePath ? getExtension(filePath) : '';
  const isMarkdown = MARKDOWN_EXTENSIONS.has(ext);
  const isImage = IMAGE_EXTENSIONS.has(ext);
  const hlLanguage = filePath ? detectLanguage(filePath) : null;

  // Build image URL for binary image files
  const imageUrl = useMemo(() => {
    if (!isImage || !filePath || !repoId) return null;
    const params = new URLSearchParams({ path: filePath });
    if (browseRef) params.set('ref', browseRef);
    return `/api/v1/repos/${repoId}/blob?${params.toString()}`;
  }, [isImage, filePath, repoId, browseRef]);

  const handleDownload = useCallback(() => {
    if (!filePath) return;
    const fileName = filePath.split('/').pop() || 'file';
    if (data && !data.is_binary) {
      const blob = new Blob([data.content], { type: 'application/octet-stream' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = fileName;
      a.click();
      URL.revokeObjectURL(url);
    } else if (imageUrl) {
      // Binary/image files: download via the blob endpoint URL
      const a = document.createElement('a');
      a.href = imageUrl;
      a.download = fileName;
      a.click();
    }
  }, [data, filePath, imageUrl]);

  // Convert markdown to HTML using marked, then sanitize through DOMPurify to
  // prevent XSS from malicious content embedded in .md files in the repository.
  const markdownHtml = useMemo(() => {
    if (!isMarkdown || !data || data.is_binary) return '';
    if (data.size_bytes > 512_000) return '';
    try {
      const raw = marked.parse(data.content) as string;
      return DOMPurify.sanitize(raw);
    } catch {
      return '';
    }
  }, [data, isMarkdown]);

  // Pre-split content into lines (guarded: returns [] when data is absent or binary)
  const lines = useMemo<string[]>(() => {
    if (!data || data.is_binary) return [];
    const rawLines = data.content.split('\n');
    return rawLines.length > 1 && rawLines[rawLines.length - 1] === ''
      ? rawLines.slice(0, -1)
      : rawLines;
  }, [data]);

  // Syntax-highlighted HTML per line. null means fall back to plain text.
  // Only attempt highlighting for text files under 512 KB.
  const highlightedLines = useMemo<string[] | null>(() => {
    if (!data || data.is_binary || !hlLanguage || data.size_bytes > 512_000) return null;
    const highlighted = highlightLines(data.content, hlLanguage);
    if (!highlighted) return null;
    // Align with the lines array (trim trailing empty entry produced by split)
    const trimmed = lines.length < highlighted.length
      ? highlighted.slice(0, lines.length)
      : highlighted;
    // Pre-sanitize each line so DOMPurify runs once per highlight computation,
    // not on every render cycle inside the .map() loop.
    return trimmed.map((html) => DOMPurify.sanitize(html));
  }, [data, hlLanguage, lines]);

  // Breadcrumb segments for the current file path
  const breadcrumbSegments = useMemo(() => {
    if (!filePath) return [];
    const parts = filePath.split('/');
    return parts.map((part, idx) => ({
      name: part,
      path: parts.slice(0, idx + 1).join('/'),
      isLast: idx === parts.length - 1,
    }));
  }, [filePath]);

  const renderBreadcrumbs = () => {
    if (breadcrumbSegments.length === 0) return null;
    return (
      <div className="flex items-center gap-0.5 font-mono text-xs overflow-hidden">
        {breadcrumbSegments.map((seg, idx) => (
          <span key={seg.path} className="flex items-center gap-0.5 min-w-0">
            {idx > 0 && <span className="text-text-muted/50 flex-shrink-0">/</span>}
            {seg.isLast ? (
              <span className="font-semibold text-accent truncate">{seg.name}</span>
            ) : (
              <button
                onClick={() => onNavigateToDir?.(seg.path)}
                className="text-text-secondary hover:text-accent transition-colors truncate"
                title={seg.path}
              >
                {seg.name}
              </button>
            )}
          </span>
        ))}
      </div>
    );
  };

  if (!filePath) {
    return (
      <div className="flex h-full items-center justify-center text-text-muted">
        <p className="text-sm">Select a file to view its contents</p>
      </div>
    );
  }

  // File history panel — shown as an overlay when requested
  if (showHistory) {
    return (
      <FileHistory
        repoId={repoId}
        filePath={filePath}
        onClose={() => setShowHistory(false)}
      />
    );
  }

  if (isLoading) {
    return <LoadingSpinner className="h-full" message="Loading file..." />;
  }

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-status-deleted">
        <FileWarning size={32} />
        <p className="text-sm">Failed to load file</p>
      </div>
    );
  }

  if (!data) return null;

  // Image viewer
  if (data.is_binary && isImage && imageUrl) {
    return (
      <div className="flex h-full flex-col">
        <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-navy-900 px-4 py-1.5">
          <div className="flex items-center gap-2 min-w-0">
            <ImageIcon size={14} className="text-accent flex-shrink-0" />
            {renderBreadcrumbs()}
          </div>
          <div className="flex items-center gap-3 flex-shrink-0">
            {imageDimensions && (
              <span className="text-[11px] text-text-muted">
                {imageDimensions.width} x {imageDimensions.height}
              </span>
            )}
            <span className="text-xs text-text-muted">{formatBytes(data.size_bytes)}</span>
            <button
              onClick={handleDownload}
              className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Download file"
              aria-label="Download file"
            >
              <Download size={11} />
              Download
            </button>
            <div className="flex items-center gap-1 border-l border-border pl-2">
              <button
                onClick={() => setImageZoom((z) => Math.max(25, z - 25))}
                className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                title="Zoom out"
                aria-label="Zoom out"
              >
                <ZoomOut size={14} />
              </button>
              <span className="min-w-[3ch] text-center text-[11px] text-text-muted">{imageZoom}%</span>
              <button
                onClick={() => setImageZoom((z) => Math.min(400, z + 25))}
                className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                title="Zoom in"
                aria-label="Zoom in"
              >
                <ZoomIn size={14} />
              </button>
              <button
                onClick={() => setImageZoom(100)}
                className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                title="Actual size"
                aria-label="Actual size"
              >
                <Maximize2 size={14} />
              </button>
            </div>
          </div>
        </div>
        <div className="flex flex-1 items-center justify-center overflow-auto bg-[repeating-conic-gradient(var(--theme-checker-dark)_0%_25%,var(--theme-checker-light)_0%_50%)] bg-[length:20px_20px] p-4">
          <img
            src={imageUrl}
            alt={data.path}
            style={{ width: `${imageZoom}%`, maxWidth: imageZoom > 100 ? 'none' : '100%' }}
            className="object-contain"
            onLoad={(e) => {
              const img = e.currentTarget;
              setImageDimensions({ width: img.naturalWidth, height: img.naturalHeight });
            }}
          />
        </div>
      </div>
    );
  }

  if (data.is_binary) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-text-muted">
        <FileWarning size={32} />
        <p className="text-sm">Binary file ({formatBytes(data.size_bytes)})</p>
      </div>
    );
  }

  // Determine whether file is text-editable (not binary, not browsing historical ref)
  const isEditable = !data.is_binary && !browseRef;

  // Delete confirmation dialog
  if (showDeleteConfirm) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="w-full max-w-sm rounded-lg border border-border bg-navy-900 shadow-2xl">
          <div className="flex items-center gap-2 border-b border-border px-4 py-3">
            <Trash2 size={16} className="text-status-deleted" />
            <h2 className="text-sm font-semibold text-text-primary">Delete File</h2>
          </div>
          <div className="p-4">
            <p className="text-sm text-text-secondary">
              Are you sure you want to delete{' '}
              <span className="font-mono font-semibold text-text-primary">{data.path}</span>?
            </p>
            <p className="mt-1 text-xs text-text-muted">This action cannot be undone.</p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={() => setShowDeleteConfirm(false)}
                className="rounded border border-border px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:bg-surface-hover"
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                disabled={deleteBlob.isPending}
                className="rounded bg-status-deleted px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-status-deleted/80 disabled:opacity-50"
              >
                {deleteBlob.isPending ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // Editing mode — full-height textarea
  if (isEditing) {
    return (
      <div className="flex h-full flex-col">
        <div className="flex items-center justify-between border-b border-border bg-navy-900 px-4 py-1.5">
          <div className="flex items-center gap-2">
            <Pencil size={12} className="text-accent" />
            <span className="font-mono text-xs text-text-secondary">{data.path}</span>
            <span className="rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-bold text-accent">EDITING</span>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCancelEdit}
              className="flex items-center gap-1 rounded border border-border px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            >
              <X size={11} />
              Cancel
            </button>
            <button
              onClick={handleSaveEdit}
              disabled={putBlob.isPending}
              className="flex items-center gap-1 rounded bg-accent px-2 py-0.5 text-[11px] font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
            >
              <Save size={11} />
              {putBlob.isPending ? 'Saving...' : 'Save'}
            </button>
          </div>
        </div>
        <textarea
          value={editContent}
          onChange={(e) => setEditContent(e.target.value)}
          aria-label="File content editor"
          className="flex-1 resize-none border-none bg-navy-950 p-4 font-mono text-[13px] leading-5 text-text-primary focus:outline-none"
          spellCheck={false}
          autoFocus
        />
      </div>
    );
  }

  // Markdown preview mode
  if (isMarkdown && viewMode === 'preview') {
    return (
      <div className="flex h-full flex-col">
        <div className="flex items-center justify-between border-b border-border bg-navy-900 px-4 py-1.5">
          <div className="min-w-0">{renderBreadcrumbs()}</div>
          <div className="flex items-center gap-2 flex-shrink-0">
            <span className="text-xs text-text-muted">{formatBytes(data.size_bytes)}</span>
            <button
              onClick={handleDownload}
              className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Download file"
              aria-label="Download file"
            >
              <Download size={11} />
              Download
            </button>
            {isEditable && (
              <>
                <button
                  onClick={handleStartEdit}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                  title="Edit file"
                  aria-label="Edit file"
                >
                  <Pencil size={11} />
                  Edit
                </button>
                <button
                  onClick={() => setShowDeleteConfirm(true)}
                  className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-status-deleted/10 hover:text-status-deleted"
                  title="Delete file"
                  aria-label="Delete file"
                >
                  <Trash2 size={11} />
                </button>
              </>
            )}
            <button
              onClick={() => setShowHistory(true)}
              className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
              title="Show file history"
              aria-label="Show file history"
            >
              <History size={11} />
              History
            </button>
            <div className="flex rounded border border-border">
              <button
                onClick={() => setViewMode('preview')}
                className="flex items-center gap-1 rounded-l px-2 py-0.5 text-[11px] font-medium bg-accent/15 text-accent"
                aria-label="Preview"
              >
                <Eye size={11} />
                Preview
              </button>
              <button
                onClick={() => setViewMode('raw')}
                className="flex items-center gap-1 rounded-r px-2 py-0.5 text-[11px] font-medium text-text-muted hover:text-text-primary"
                aria-label="Raw"
              >
                <Code size={11} />
                Raw
              </button>
            </div>
          </div>
        </div>
        <div className="flex-1 overflow-auto">
          <div className="mx-auto max-w-4xl p-6">
            {markdownHtml ? (
              <div
                className="markdown-body"
                dangerouslySetInnerHTML={{ __html: markdownHtml }}
              />
            ) : (
              <p className="text-sm text-text-muted">File too large for markdown preview. Switch to Raw view.</p>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-navy-900 px-4 py-1.5">
        <div className="min-w-0">{renderBreadcrumbs()}</div>
        <div className="flex items-center gap-2 flex-shrink-0">
          <span className="text-xs text-text-muted">{formatBytes(data.size_bytes)}</span>
          <button
            onClick={handleDownload}
            className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            title="Download file"
            aria-label="Download file"
          >
            <Download size={11} />
            Download
          </button>
          {isEditable && (
            <>
              <button
                onClick={handleStartEdit}
                className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
                title="Edit file"
                aria-label="Edit file"
              >
                <Pencil size={11} />
                Edit
              </button>
              <button
                onClick={() => setShowDeleteConfirm(true)}
                className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-status-deleted/10 hover:text-status-deleted"
                title="Delete file"
                aria-label="Delete file"
              >
                <Trash2 size={11} />
              </button>
            </>
          )}
          <button
            onClick={() => setShowHistory(true)}
            className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
            title="Show file history"
            aria-label="Show file history"
          >
            <History size={11} />
            History
          </button>
          {isMarkdown && (
            <div className="flex rounded border border-border">
              <button
                onClick={() => setViewMode('preview')}
                className="flex items-center gap-1 rounded-l px-2 py-0.5 text-[11px] font-medium text-text-muted hover:text-text-primary"
                aria-label="Preview"
              >
                <Eye size={11} />
                Preview
              </button>
              <button
                onClick={() => setViewMode('raw')}
                className="flex items-center gap-1 rounded-r px-2 py-0.5 text-[11px] font-medium bg-accent/15 text-accent"
                aria-label="Raw"
              >
                <Code size={11} />
                Raw
              </button>
            </div>
          )}
        </div>
      </div>
      {/* Large file warnings */}
      {data.size_bytes > LARGE_FILE_SIZE_LIMIT && (
        <div className="flex items-center gap-2 border-b border-border bg-amber-500/10 px-4 py-1.5">
          <AlertTriangle size={13} className="flex-shrink-0 text-amber-400" />
          <span className="text-xs text-amber-300">
            Very large file ({formatBytes(data.size_bytes)}) — only first {LINE_CAP.toLocaleString()} lines shown
          </span>
        </div>
      )}
      {data.size_bytes > HIGHLIGHT_SIZE_LIMIT && data.size_bytes <= LARGE_FILE_SIZE_LIMIT && (
        <div className="flex items-center gap-2 border-b border-border bg-blue-500/10 px-4 py-1.5">
          <Info size={13} className="flex-shrink-0 text-blue-400" />
          <span className="text-xs text-blue-300">
            Large file — syntax highlighting disabled for performance
          </span>
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse font-mono text-[13px] leading-5">
          <tbody>
            {lines.slice(0, visibleLineCap).map((line, idx) => {
              const lineNum = idx + 1;
              const isHighlighted = highlightLine === lineNum;
              const hlHtml = highlightedLines?.[idx];
              return (
                <tr
                  key={idx}
                  ref={isHighlighted ? highlightRef : undefined}
                  className={isHighlighted ? 'bg-accent/20' : 'hover:bg-surface-hover/50'}
                >
                  <td className={`w-12 select-none border-r border-border px-3 text-right ${
                    isHighlighted ? 'text-accent' : 'text-text-muted/50'
                  }`}>
                    {lineNum}
                  </td>
                  {hlHtml !== undefined ? (
                    <td
                      className="whitespace-pre px-4"
                      dangerouslySetInnerHTML={{ __html: hlHtml || '\u00A0' }}
                    />
                  ) : (
                    <td className="whitespace-pre px-4">
                      {line || '\u00A0'}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
        {lines.length > visibleLineCap && (
          <div className="border-t border-border px-4 py-2 text-center">
            <button
              onClick={() => setVisibleLineCap((cap) => cap + LINE_CAP)}
              className="rounded px-3 py-1 text-xs font-medium text-accent transition-colors hover:bg-accent/10"
            >
              Show more ({Math.min(LINE_CAP, lines.length - visibleLineCap).toLocaleString()} of{' '}
              {(lines.length - visibleLineCap).toLocaleString()} remaining)
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

export default FileViewer;
