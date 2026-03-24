import { useState, useCallback, useMemo } from 'react';
import type { ReactNode } from 'react';
import { ChevronRight, ChevronDown, Folder, File, FileCode, FileText, FileImage, Eye, Clipboard, FoldVertical, UnfoldVertical, FileSearch, History, FilePlus, FolderPlus, Trash2, Pencil } from 'lucide-react';
import { useFileTree, usePutBlob, useCreateDirectory, useDeleteBlob, useMoveFile } from '../hooks/useRepo.ts';
import { useParams, useNavigate } from 'react-router-dom';
import type { FileStatusEntry } from '../api/types.ts';
import ContextMenu from './ContextMenu.tsx';
import type { ContextMenuItem } from './ContextMenu.tsx';
import { useToast } from '../contexts/ToastContext.tsx';

interface FileTreeProps {
  repoId: string;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  onShowFileHistory?: (path: string) => void;
  statusEntries?: FileStatusEntry[];
  browseRef?: string;
}

interface ContextMenuState {
  items: ContextMenuItem[];
  position: { x: number; y: number };
}

function FileTree({ repoId, selectedPath, onSelectFile, onShowFileHistory, statusEntries = [], browseRef }: FileTreeProps) {
  const { repoId: routeRepoId } = useParams<{ repoId: string }>();
  const navigate = useNavigate();
  const toast = useToast();
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const effectiveRepoId = repoId || routeRepoId;

  const putBlobMutation = usePutBlob(effectiveRepoId ?? '');
  const createDirMutation = useCreateDirectory(effectiveRepoId ?? '');
  const deleteBlobMutation = useDeleteBlob(effectiveRepoId ?? '');
  const moveFileMutation = useMoveFile(effectiveRepoId ?? '');

  const handleFileContextMenu = useCallback(
    (e: React.MouseEvent, path: string) => {
      e.preventDefault();
      const items: ContextMenuItem[] = [
        {
          label: 'View File',
          icon: <Eye size={13} />,
          onClick: () => onSelectFile(path),
        },
        {
          label: 'File History',
          icon: <History size={13} />,
          onClick: () => onShowFileHistory?.(path),
        },
        {
          label: 'Copy Path',
          icon: <Clipboard size={13} />,
          onClick: () => {
            void navigator.clipboard.writeText(path);
          },
        },
        {
          label: 'Blame',
          icon: <FileSearch size={13} />,
          onClick: () => {
            if (effectiveRepoId) {
              navigate(`/repo/${effectiveRepoId}/blame/${path}`);
            }
          },
        },
        {
          label: 'Rename',
          icon: <Pencil size={13} />,
          onClick: () => {
            const fileName = path.split('/').pop() ?? path;
            const newName = window.prompt('New name:', fileName);
            if (newName?.trim() && newName.trim() !== fileName) {
              const parentDir = path.includes('/') ? path.substring(0, path.lastIndexOf('/') + 1) : '';
              const toPath = `${parentDir}${newName.trim()}`;
              moveFileMutation.mutate(
                { fromPath: path, toPath },
                {
                  onSuccess: () => toast.success(`Renamed to ${toPath}`),
                  onError: (err: Error) => toast.error(`Failed to rename: ${err.message}`),
                },
              );
            }
          },
        },
        {
          label: 'Delete File',
          icon: <Trash2 size={13} />,
          danger: true,
          onClick: () => {
            if (window.confirm(`Delete "${path}"? This cannot be undone.`)) {
              deleteBlobMutation.mutate(path, {
                onSuccess: () => toast.success(`Deleted ${path}`),
                onError: (err: Error) => toast.error(`Failed to delete: ${err.message}`),
              });
            }
          },
        },
      ];
      setContextMenu({
        position: { x: e.clientX, y: e.clientY },
        items,
      });
    },
    [onSelectFile, onShowFileHistory, effectiveRepoId, navigate, deleteBlobMutation, moveFileMutation, toast],
  );

  const handleDirContextMenu = useCallback(
    (e: React.MouseEvent, path: string) => {
      e.preventDefault();
      const dirPrefix = path ? `${path}/` : '';
      setContextMenu({
        position: { x: e.clientX, y: e.clientY },
        items: [
          {
            label: 'New File Here',
            icon: <FilePlus size={13} />,
            onClick: () => {
              const name = window.prompt('File name:');
              if (name?.trim()) {
                const fullPath = `${dirPrefix}${name.trim()}`;
                putBlobMutation.mutate(
                  { path: fullPath, content: '' },
                  {
                    onSuccess: () => {
                      toast.success(`Created ${fullPath}`);
                      onSelectFile(fullPath);
                    },
                    onError: (err: Error) => toast.error(`Failed to create file: ${err.message}`),
                  },
                );
              }
            },
          },
          {
            label: 'New Folder Here',
            icon: <FolderPlus size={13} />,
            onClick: () => {
              const name = window.prompt('Folder name:');
              if (name?.trim()) {
                const fullPath = `${dirPrefix}${name.trim()}`;
                createDirMutation.mutate(fullPath, {
                  onSuccess: () => toast.success(`Created directory ${fullPath}`),
                  onError: (err: Error) => toast.error(`Failed to create directory: ${err.message}`),
                });
              }
            },
          },
          {
            label: 'Rename',
            icon: <Pencil size={13} />,
            onClick: () => {
              const dirName = path.split('/').pop() ?? path;
              const newName = window.prompt('New name:', dirName);
              if (newName?.trim() && newName.trim() !== dirName) {
                const parentDir = path.includes('/') ? path.substring(0, path.lastIndexOf('/') + 1) : '';
                const toPath = `${parentDir}${newName.trim()}`;
                moveFileMutation.mutate(
                  { fromPath: path, toPath },
                  {
                    onSuccess: () => toast.success(`Renamed to ${toPath}`),
                    onError: (err: Error) => toast.error(`Failed to rename: ${err.message}`),
                  },
                );
              }
            },
          },
          {
            label: 'Copy Path',
            icon: <Clipboard size={13} />,
            onClick: () => {
              void navigator.clipboard.writeText(path);
            },
          },
        ],
      });
    },
    [putBlobMutation, createDirMutation, moveFileMutation, toast, onSelectFile],
  );

  return (
    <div className="h-full overflow-y-auto text-sm">
      <DirectoryNode
        repoId={repoId}
        path=""
        depth={0}
        selectedPath={selectedPath}
        onSelectFile={onSelectFile}
        statusEntries={statusEntries}
        defaultExpanded
        onFileContextMenu={handleFileContextMenu}
        onDirContextMenu={handleDirContextMenu}
        browseRef={browseRef}
      />
      {contextMenu && (
        <ContextMenu
          items={contextMenu.items}
          position={contextMenu.position}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}

interface DirectoryNodeProps {
  repoId: string;
  path: string;
  depth: number;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  statusEntries: FileStatusEntry[];
  defaultExpanded?: boolean;
  onFileContextMenu: (e: React.MouseEvent, path: string) => void;
  onDirContextMenu: (e: React.MouseEvent, path: string) => void;
  browseRef?: string;
}

function DirectoryNode({
  repoId,
  path,
  depth,
  selectedPath,
  onSelectFile,
  statusEntries,
  onFileContextMenu,
  onDirContextMenu,
  browseRef,
}: DirectoryNodeProps) {
  const { data: entries, isLoading } = useFileTree(repoId, path || undefined, browseRef);

  if (isLoading) {
    return (
      <div style={{ paddingLeft: depth * 16 + 8 }} className="py-1 text-xs text-text-muted">
        Loading...
      </div>
    );
  }

  return (
    <div>
      {entries?.map((entry) => {
        const isDir = entry.entry_type === 'directory';
        const status = statusEntries.find((s) => s.path === entry.path);

        if (isDir) {
          return (
            <DirectoryEntry
              key={entry.path}
              repoId={repoId}
              name={entry.name}
              path={entry.path}
              depth={depth}
              selectedPath={selectedPath}
              onSelectFile={onSelectFile}
              statusEntries={statusEntries}
              onFileContextMenu={onFileContextMenu}
              onDirContextMenu={onDirContextMenu}
              browseRef={browseRef}
            />
          );
        }

        return (
          <FileEntry
            key={entry.path}
            name={entry.name}
            path={entry.path}
            depth={depth}
            isSelected={selectedPath === entry.path}
            onSelect={onSelectFile}
            status={status?.status}
            onContextMenu={onFileContextMenu}
          />
        );
      })}
    </div>
  );
}

interface DirectoryEntryProps {
  repoId: string;
  name: string;
  path: string;
  depth: number;
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  statusEntries: FileStatusEntry[];
  onFileContextMenu: (e: React.MouseEvent, path: string) => void;
  onDirContextMenu: (e: React.MouseEvent, path: string) => void;
  browseRef?: string;
}

function DirectoryEntry({
  repoId,
  name,
  path,
  depth,
  selectedPath,
  onSelectFile,
  statusEntries,
  onFileContextMenu,
  onDirContextMenu,
  browseRef,
}: DirectoryEntryProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div>
      <button
        onClick={() => setExpanded(!expanded)}
        onContextMenu={(e) => onDirContextMenu(e, path)}
        style={{ paddingLeft: depth * 16 + 4 }}
        className="group/dir flex w-full items-center gap-1 rounded py-0.5 pr-2 text-left text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary"
      >
        {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <Folder size={14} className="text-accent/70" />
        <span className="flex-1 truncate">{name}</span>
        <span className="flex gap-0.5 opacity-0 transition-opacity group-hover/dir:opacity-100">
          {expanded ? (
            <span
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(false);
              }}
              className="rounded p-0.5 text-text-muted hover:text-accent"
              title="Collapse"
            >
              <FoldVertical size={11} />
            </span>
          ) : (
            <span
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(true);
              }}
              className="rounded p-0.5 text-text-muted hover:text-accent"
              title="Expand"
            >
              <UnfoldVertical size={11} />
            </span>
          )}
        </span>
      </button>
      {expanded && (
        <DirectoryNode
          repoId={repoId}
          path={path}
          depth={depth + 1}
          selectedPath={selectedPath}
          onSelectFile={onSelectFile}
          statusEntries={statusEntries}
          onFileContextMenu={onFileContextMenu}
          onDirContextMenu={onDirContextMenu}
          browseRef={browseRef}
        />
      )}
    </div>
  );
}

interface FileEntryProps {
  name: string;
  path: string;
  depth: number;
  isSelected: boolean;
  onSelect: (path: string) => void;
  status?: string;
  onContextMenu: (e: React.MouseEvent, path: string) => void;
}

function FileEntry({ name, path, depth, isSelected, onSelect, status, onContextMenu }: FileEntryProps) {
  const icon = useMemo(() => getFileIcon(name), [name]);
  const statusBadge = getStatusBadge(status);

  return (
    <button
      onClick={() => onSelect(path)}
      onContextMenu={(e) => onContextMenu(e, path)}
      style={{ paddingLeft: depth * 16 + 22 }}
      className={`flex w-full items-center gap-1.5 rounded py-0.5 pr-2 text-left transition-colors ${
        isSelected
          ? 'bg-accent/15 text-accent'
          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
      }`}
    >
      {icon}
      <span className="truncate">{name}</span>
      {statusBadge && (
        <span
          className={`ml-auto flex-shrink-0 rounded px-1 text-[10px] font-bold ${statusBadge.className}`}
        >
          {statusBadge.label}
        </span>
      )}
    </button>
  );
}

function getFileIcon(name: string): ReactNode {
  const ext = name.split('.').pop()?.toLowerCase();
  const props = { size: 14, className: "flex-shrink-0 text-text-muted" };
  switch (ext) {
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'rs':
    case 'py':
    case 'go':
    case 'java':
    case 'c':
    case 'cpp':
    case 'h':
    case 'rb':
      return <FileCode {...props} />;
    case 'md':
    case 'txt':
    case 'toml':
    case 'yaml':
    case 'yml':
    case 'json':
    case 'xml':
    case 'html':
    case 'css':
      return <FileText {...props} />;
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'svg':
    case 'webp':
      return <FileImage {...props} />;
    default:
      return <File {...props} />;
  }
}

function getStatusBadge(status: string | undefined): { label: string; className: string } | null {
  switch (status) {
    case 'added':
      return { label: 'A', className: 'text-status-added bg-status-added/15' };
    case 'modified':
      return { label: 'M', className: 'text-status-modified bg-status-modified/15' };
    case 'deleted':
      return { label: 'D', className: 'text-status-deleted bg-status-deleted/15' };
    case 'renamed':
      return { label: 'R', className: 'text-purple-400 bg-purple-400/15' };
    case 'copied':
      return { label: 'C', className: 'text-blue-400 bg-blue-400/15' };
    case 'conflicted':
      return { label: '!', className: 'text-orange-400 bg-orange-400/15' };
    default:
      return null;
  }
}

export default FileTree;
