import { useEffect, useState, useRef, useCallback } from 'react';
import { useNavigate, useParams, useLocation } from 'react-router-dom';
import {
  GitBranch,
  GitPullRequest,
  Plus,
  Settings,
  Clock,
  Home,
  Zap,
  Search,
  RotateCcw,
  GitCompareArrows,
  FolderTree,
  ChevronDown,
  Sun,
  Moon,
  Package,
  PanelLeftClose,
  PanelLeftOpen,
  BookOpen,
  ShieldCheck,
  LayoutDashboard,
} from 'lucide-react';
import { useRepos } from '../hooks/useRepo.ts';
import { useCommandPalette } from '../contexts/CommandPaletteContext.tsx';
import { useTheme } from '../contexts/ThemeContext.tsx';
import type { PaletteCommand } from '../contexts/CommandPaletteContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';

const SIDEBAR_COLLAPSED_KEY = 'ovc_sidebar_collapsed';
const MOBILE_BREAKPOINT = '(min-width: 768px)';

function useSidebarCollapsed() {
  const [collapsed, setCollapsed] = useState(() => {
    // On mobile viewports, default to collapsed regardless of stored preference
    if (typeof window !== 'undefined' && !window.matchMedia(MOBILE_BREAKPOINT).matches) {
      return true;
    }
    try {
      return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true';
    } catch {
      return false;
    }
  });

  // Auto-collapse when viewport shrinks below the mobile breakpoint
  useEffect(() => {
    const mq = window.matchMedia(MOBILE_BREAKPOINT);
    function handleChange(e: MediaQueryListEvent) {
      if (!e.matches) {
        // Dropped below 768px — collapse immediately
        setCollapsed(true);
      }
    }
    mq.addEventListener('change', handleChange);
    return () => mq.removeEventListener('change', handleChange);
  }, []);

  const toggle = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      try {
        localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(next));
      } catch {
        // localStorage may be unavailable
      }
      return next;
    });
  }, []);

  const collapse = useCallback(() => setCollapsed(true), []);

  return { collapsed, toggle, collapse } as const;
}

interface SidebarProps {
  onCreateRepo: () => void;
}

function Sidebar({ onCreateRepo }: SidebarProps) {
  const navigate = useNavigate();
  const { repoId } = useParams<{ repoId: string }>();
  const location = useLocation();
  const { data: repos, isLoading } = useRepos();
  const { registerCommands, unregisterCommands } = useCommandPalette();
  const { theme, toggleTheme } = useTheme();
  const [showRepoSwitcher, setShowRepoSwitcher] = useState(false);
  const switcherRef = useRef<HTMLDivElement>(null);
  const { collapsed, toggle: toggleCollapsed, collapse } = useSidebarCollapsed();
  const isMobile = typeof window !== 'undefined' && !window.matchMedia(MOBILE_BREAKPOINT).matches;

  useEffect(() => {
    const commands: PaletteCommand[] = [
      {
        id: 'nav-repositories',
        label: 'Go to Repositories',
        category: 'Navigation',
        action: () => navigate('/'),
      },
    ];

    if (repoId) {
      commands.push(
        // Navigation
        {
          id: 'nav-overview',
          label: 'Go to Overview',
          category: 'Navigation',
          action: () => navigate(`/repo/${repoId}/overview`),
        },
        {
          id: 'nav-history',
          label: 'Go to History',
          category: 'Navigation',
          shortcut: 'G H',
          action: () => navigate(`/repo/${repoId}/history`),
        },
        {
          id: 'nav-pulls',
          label: 'Go to Pull Requests',
          category: 'Navigation',
          shortcut: 'G P',
          action: () => navigate(`/repo/${repoId}/pulls`),
        },
        {
          id: 'nav-actions',
          label: 'Go to Actions',
          category: 'Navigation',
          shortcut: 'G A',
          action: () => navigate(`/repo/${repoId}/actions`),
        },
        {
          id: 'nav-search',
          label: 'Search Files',
          category: 'Search',
          shortcut: 'G S',
          action: () => navigate(`/repo/${repoId}/search`),
        },
        {
          id: 'nav-diff-staged',
          label: 'View Staged Diff',
          category: 'Git',
          action: () => navigate(`/repo/${repoId}/diff`),
        },
        {
          id: 'nav-diff-compare',
          label: 'Compare Branches / Refs',
          category: 'Git',
          action: () => navigate(`/repo/${repoId}/diff`),
        },
        {
          id: 'nav-reflog',
          label: 'Go to Reflog',
          category: 'Navigation',
          action: () => navigate(`/repo/${repoId}/reflog`),
        },
        {
          id: 'nav-dependencies',
          label: 'Check Dependencies',
          category: 'Navigation',
          shortcut: 'G D',
          action: () => navigate(`/repo/${repoId}/dependencies`),
        },
        {
          id: 'nav-access',
          label: 'Manage Access Control',
          category: 'Settings',
          action: () => navigate(`/repo/${repoId}/access`),
        },
        {
          id: 'nav-settings',
          label: 'Go to Settings',
          category: 'Settings',
          action: () => navigate(`/repo/${repoId}/settings`),
        },
      );
    }

    commands.push({
      id: 'nav-docs',
      label: 'Go to Documentation',
      category: 'Navigation',
      action: () => { window.location.href = '/docs'; },
    });

    registerCommands(commands);
    const ids = commands.map((c) => c.id);
    return () => unregisterCommands(ids);
  }, [repoId, navigate, registerCommands, unregisterCommands]);

  const currentRepo = repos?.find((r) => r.id === repoId);

  // When inside a repo, show compact sidebar with nav links as primary content
  if (repoId) {
    interface NavLink {
      to: string;
      label: string;
      icon: typeof FolderTree;
      exact: boolean;
      hash?: string;
    }

    const navLinks: NavLink[] = [
      { to: `/repo/${repoId}/overview`, label: 'Overview', icon: LayoutDashboard, exact: false },
      { to: `/repo/${repoId}`, label: 'Files', icon: FolderTree, exact: true },
      { to: `/repo/${repoId}/history`, label: 'History', icon: Clock, exact: false },
      { to: `/repo/${repoId}/pulls`, label: 'Pull Requests', icon: GitPullRequest, exact: false },
      { to: `/repo/${repoId}/actions`, label: 'Actions', icon: Zap, exact: false },
      { to: `/repo/${repoId}/dependencies`, label: 'Dependencies', icon: Package, exact: false },
      { to: `/repo/${repoId}/search`, label: 'Search', icon: Search, exact: false },
      { to: `/repo/${repoId}/diff`, label: 'Diff', icon: GitCompareArrows, exact: false },
      { to: `/repo/${repoId}/access`, label: 'Access', icon: ShieldCheck, exact: false },
      { to: `/repo/${repoId}/reflog`, label: 'Reflog', icon: RotateCcw, exact: false },
      { to: `/repo/${repoId}/settings`, label: 'Settings', icon: Settings, exact: false },
    ];

    return (
      <>
        {/* Mobile backdrop — clicking collapses the sidebar */}
        {!collapsed && isMobile && (
          <div
            className="fixed inset-0 z-20 bg-navy-950/60"
            onClick={collapse}
            aria-hidden="true"
          />
        )}
      <aside
        className={`flex h-full flex-shrink-0 flex-col border-r border-border bg-navy-900 transition-[width] duration-200 ease-in-out ${
          collapsed ? 'w-12' : 'w-48'
        } ${!collapsed && isMobile ? 'fixed left-0 top-0 z-30 h-screen' : ''}`}
      >
        {/* Logo + collapse toggle */}
        <div className="flex items-center gap-2 border-b border-border px-3 py-3">
          <div className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-md bg-accent/20">
            <GitBranch size={14} className="text-accent" />
          </div>
          {!collapsed && <span className="text-sm font-semibold tracking-wide text-text-primary">OVC</span>}
          <button
            onClick={toggleCollapsed}
            className={`rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${collapsed ? '' : 'ml-auto'}`}
            title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            {collapsed ? <PanelLeftOpen size={14} /> : <PanelLeftClose size={14} />}
          </button>
        </div>

        {/* Repo switcher */}
        {!collapsed && (
          <div className="relative border-b border-border px-2 py-2" ref={switcherRef}>
            <button
              onClick={() => setShowRepoSwitcher(!showRepoSwitcher)}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-surface-hover"
            >
              <div className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded bg-accent/15 text-[10px] font-bold text-accent">
                {currentRepo?.name.slice(0, 2).toUpperCase() ?? '??'}
              </div>
              <span className="min-w-0 flex-1 truncate text-xs font-semibold text-text-primary">
                {currentRepo?.name ?? repoId}
              </span>
              <ChevronDown size={12} className="flex-shrink-0 text-text-muted" />
            </button>

            {showRepoSwitcher && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={() => setShowRepoSwitcher(false)}
                />
                <div className="absolute left-2 right-2 top-full z-20 mt-1 rounded-md border border-border bg-navy-800 py-1 shadow-lg">
                  <button
                    onClick={() => {
                      navigate('/');
                      setShowRepoSwitcher(false);
                    }}
                    className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary"
                  >
                    <Home size={12} />
                    All Repositories
                  </button>
                  <div className="my-1 border-t border-border" />
                  {repos?.map((repo) => (
                    <button
                      key={repo.id}
                      onClick={() => {
                        navigate(`/repo/${repo.id}`);
                        setShowRepoSwitcher(false);
                      }}
                      className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                        repo.id === repoId
                          ? 'bg-accent/10 text-accent'
                          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                      }`}
                    >
                      <div className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded bg-accent/15 text-[9px] font-bold text-accent">
                        {repo.name.slice(0, 2).toUpperCase()}
                      </div>
                      <span className="truncate">{repo.name}</span>
                      {repo.id === repoId && (
                        <span className="ml-auto text-[10px] text-accent/60">current</span>
                      )}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        )}

        {/* Nav links — full page navigation (SPA Link doesn't re-render views correctly) */}
        <nav className={`flex-1 overflow-y-auto py-2 ${collapsed ? 'px-1' : 'px-2'}`}>
          {navLinks.map(({ to, label, icon: Icon, exact, hash }) => {
            const isActive = exact
              ? (location.pathname === to && !hash)
              : location.pathname === to || location.pathname.startsWith(`${to}/`);
            const href = hash ? `${to}?view=${hash}` : to;
            return (
              <a
                key={label}
                href={href}
                aria-label={label}
                title={collapsed ? label : undefined}
                className={`mb-0.5 flex w-full items-center rounded-md text-sm font-medium transition-colors ${
                  collapsed ? 'justify-center px-0 py-2.5' : 'gap-2.5 px-3 py-2.5'
                } ${
                  isActive
                    ? 'bg-accent/15 text-accent'
                    : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                }`}
              >
                <Icon size={16} className="flex-shrink-0" />
                {!collapsed && label}
              </a>
            );
          })}
        </nav>

        {/* Docs + Create repo + Theme toggle */}
        <div className={`border-t border-border py-2 ${collapsed ? 'px-1' : 'px-2'}`}>
          <a
            href="/docs"
            className={`flex w-full items-center rounded-md text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${
              collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
            }`}
            title="Documentation"
            aria-label="Documentation"
          >
            <BookOpen size={14} className="flex-shrink-0" />
            {!collapsed && 'Docs'}
          </a>
          <button
            onClick={onCreateRepo}
            className={`flex w-full items-center rounded-md text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${
              collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
            }`}
            title="Create repository"
            aria-label="Create repository"
          >
            <Plus size={14} className="flex-shrink-0" />
            {!collapsed && 'New Repository'}
          </button>
          <button
            onClick={toggleTheme}
            className={`flex w-full items-center rounded-md text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${
              collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
            }`}
            title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
            aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
          >
            {theme === 'dark' ? <Sun size={14} className="flex-shrink-0" /> : <Moon size={14} className="flex-shrink-0" />}
            {!collapsed && (theme === 'dark' ? 'Light Mode' : 'Dark Mode')}
          </button>
        </div>
      </aside>
      </>
    );
  }

  // Default sidebar for repo list page
  return (
    <>
      {/* Mobile backdrop — clicking collapses the sidebar */}
      {!collapsed && isMobile && (
        <div
          className="fixed inset-0 z-20 bg-navy-950/60"
          onClick={collapse}
          aria-hidden="true"
        />
      )}
    <aside
      className={`flex h-full flex-shrink-0 flex-col border-r border-border bg-navy-900 transition-[width] duration-200 ease-in-out ${
        collapsed ? 'w-12' : 'w-48'
      } ${!collapsed && isMobile ? 'fixed left-0 top-0 z-30 h-screen' : ''}`}
    >
      <div className="flex items-center gap-2 border-b border-border px-3 py-3">
        <div className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-md bg-accent/20">
          <GitBranch size={14} className="text-accent" />
        </div>
        {!collapsed && <span className="text-sm font-semibold tracking-wide text-text-primary">OVC</span>}
        <button
          onClick={toggleCollapsed}
          className={`rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${collapsed ? '' : 'ml-auto'}`}
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <PanelLeftOpen size={14} /> : <PanelLeftClose size={14} />}
        </button>
      </div>

      <nav className={`flex-1 overflow-y-auto py-2 ${collapsed ? 'px-1' : 'px-2'}`}>
        <button
          onClick={() => navigate('/')}
          aria-label="Repositories"
          className={`mb-1 flex w-full items-center rounded-md text-sm font-medium text-accent bg-accent/15 transition-colors ${
            collapsed ? 'justify-center px-0 py-2.5' : 'gap-2 px-3 py-2.5'
          }`}
        >
          <Home size={16} className="flex-shrink-0" />
          {!collapsed && 'Repositories'}
        </button>

        {!collapsed && (
          <div className="mb-1 mt-3 flex items-center justify-between px-3">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              Repos
            </span>
            <button
              onClick={onCreateRepo}
              className="rounded p-0.5 text-text-muted transition-colors hover:bg-surface-hover hover:text-accent"
              title="Create repository"
              aria-label="Create repository"
            >
              <Plus size={14} />
            </button>
          </div>
        )}

        {isLoading && <LoadingSpinner size={18} className="py-4" />}

        {repos?.map((repo) => (
          <button
            key={repo.id}
            onClick={() => navigate(`/repo/${repo.id}`)}
            title={collapsed ? repo.name : undefined}
            className={`mb-0.5 flex w-full items-center rounded-md text-left text-sm text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary ${
              collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
            }`}
          >
            <div className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded bg-accent/15 text-[9px] font-bold text-accent">
              {repo.name.slice(0, 2).toUpperCase()}
            </div>
            {!collapsed && <span className="truncate">{repo.name}</span>}
          </button>
        ))}

        {!collapsed && repos && repos.length === 0 && (
          <p className="px-3 py-4 text-center text-xs text-text-muted">
            No repositories yet
          </p>
        )}
      </nav>

      <div className={`border-t border-border py-2 ${collapsed ? 'px-1' : 'px-2'}`}>
        <a
          href="/docs"
          className={`flex w-full items-center rounded-md text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${
            collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
          }`}
          title="Documentation"
          aria-label="Documentation"
        >
          <BookOpen size={14} className="flex-shrink-0" />
          {!collapsed && 'Docs'}
        </a>
        <button
          onClick={toggleTheme}
          className={`flex w-full items-center rounded-md text-xs text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary ${
            collapsed ? 'justify-center px-0 py-2' : 'gap-2 px-3 py-2'
          }`}
          title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
          aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
        >
          {theme === 'dark' ? <Sun size={14} className="flex-shrink-0" /> : <Moon size={14} className="flex-shrink-0" />}
          {!collapsed && (theme === 'dark' ? 'Light Mode' : 'Dark Mode')}
        </button>
      </div>
    </aside>
    </>
  );
}

export default Sidebar;
