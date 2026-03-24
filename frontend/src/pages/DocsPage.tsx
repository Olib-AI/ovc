import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { Search, BookOpen, ChevronRight, ChevronDown, Terminal, Monitor, FileText, AlertTriangle } from 'lucide-react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { useDocsIndex, useDocSearch, useDocSection } from '../hooks/useDocs.ts';
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut.ts';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import type { DocCategory, DocSearchResult } from '../api/types.ts';

const CATEGORY_ICONS: Record<string, typeof Terminal> = {
  cli: Terminal,
  ui: Monitor,
};

function highlightText(text: string, query: string): string {
  if (!query.trim()) return text;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const regex = new RegExp(`(${escaped})`, 'gi');
  return text.replace(regex, '<mark class="bg-accent/30 text-accent rounded px-0.5">$1</mark>');
}

function renderMarkdown(content: string): string {
  const raw = marked.parse(content, { async: false }) as string;
  return DOMPurify.sanitize(raw);
}

interface DocsSidebarProps {
  categories: DocCategory[];
  activeCategory: string | null;
  activeSection: string | null;
  expandedCategories: Set<string>;
  onToggleCategory: (id: string) => void;
  onSelectSection: (categoryId: string, sectionId: string) => void;
}

function DocsSidebar({
  categories,
  activeCategory,
  activeSection,
  expandedCategories,
  onToggleCategory,
  onSelectSection,
}: DocsSidebarProps) {
  return (
    <nav className="space-y-1 py-2">
      {categories.map((category) => {
        const isExpanded = expandedCategories.has(category.id);
        const Icon = CATEGORY_ICONS[category.id] ?? FileText;

        return (
          <div key={category.id}>
            <button
              onClick={() => onToggleCategory(category.id)}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary"
            >
              {isExpanded ? (
                <ChevronDown size={14} className="flex-shrink-0 text-text-muted" />
              ) : (
                <ChevronRight size={14} className="flex-shrink-0 text-text-muted" />
              )}
              <Icon size={14} className="flex-shrink-0 text-accent" />
              <span>{category.title}</span>
              <span className="ml-auto text-[10px] text-text-muted">
                {category.sections.length}
              </span>
            </button>

            {isExpanded && (
              <div className="ml-5 space-y-0.5 border-l border-border pl-3">
                {category.sections.map((section) => {
                  const isActive =
                    activeCategory === category.id && activeSection === section.id;
                  return (
                    <a
                      key={section.id}
                      href={`/docs?category=${category.id}&section=${section.id}`}
                      onClick={(e) => {
                        e.preventDefault();
                        onSelectSection(category.id, section.id);
                      }}
                      className={`block rounded-md px-3 py-1.5 text-xs transition-colors ${
                        isActive
                          ? 'bg-accent/15 font-medium text-accent'
                          : 'text-text-muted hover:bg-surface-hover hover:text-text-primary'
                      }`}
                    >
                      {section.title}
                    </a>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </nav>
  );
}

interface SearchResultsProps {
  results: DocSearchResult[];
  query: string;
  onSelect: (categoryId: string, sectionId: string) => void;
}

function SearchResults({ results, query, onSelect }: SearchResultsProps) {
  if (results.length === 0) {
    return (
      <div className="p-8 text-center text-sm text-text-muted">
        No results found for &ldquo;{query}&rdquo;
      </div>
    );
  }

  return (
    <div className="space-y-2 p-4">
      <p className="text-xs text-text-muted">
        {results.length} result{results.length !== 1 ? 's' : ''} for &ldquo;{query}&rdquo;
      </p>
      {results.map((result) => (
        <button
          key={`${result.category}-${result.section}`}
          onClick={() => onSelect(result.category, result.section)}
          className="block w-full rounded-lg border border-border bg-navy-900 p-4 text-left transition-colors hover:border-accent/30 hover:bg-surface-hover"
        >
          <div className="mb-1 flex items-center gap-2">
            <span className="rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium text-accent">
              {result.category}
            </span>
            <span className="text-sm font-medium text-text-primary">{result.title}</span>
          </div>
          <p
            className="text-xs leading-relaxed text-text-secondary"
            dangerouslySetInnerHTML={{
              __html: DOMPurify.sanitize(highlightText(result.snippet, query)),
            }}
          />
        </button>
      ))}
    </div>
  );
}

function DocsPage() {
  useDocumentTitle('Documentation \u2014 OVC');

  const [searchQuery, setSearchQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [activeCategory, setActiveCategory] = useState<string | null>(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get('category');
  });
  const [activeSection, setActiveSection] = useState<string | null>(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get('section');
  });
  const [expandedCategories, setExpandedCategories] = useState<Set<string>>(new Set());
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const initializedRef = useRef(false);

  const { data: docsIndex, isLoading: indexLoading, error: indexError } = useDocsIndex();
  const { data: searchResults, isLoading: searchLoading } = useDocSearch(debouncedQuery);
  const { data: sectionData, isLoading: sectionLoading } = useDocSection(
    activeCategory,
    activeSection,
  );

  // Debounce search
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(searchQuery), 300);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  // Auto-expand all categories and select first section on initial load
  useEffect(() => {
    if (!docsIndex?.categories || initializedRef.current) return;
    initializedRef.current = true;

    setExpandedCategories(new Set(docsIndex.categories.map((c) => c.id)));

    // Select first section if none selected (URL params already handled via initializer)
    if (!activeCategory && !activeSection && docsIndex.categories.length > 0) {
      const firstCat = docsIndex.categories[0];
      if (firstCat && firstCat.sections.length > 0) {
        const firstSection = firstCat.sections[0];
        if (firstSection) {
          setActiveCategory(firstCat.id);
          setActiveSection(firstSection.id);
        }
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docsIndex]);

  // Focus search on '/' key
  const focusSearch = useCallback(() => {
    searchInputRef.current?.focus();
  }, []);
  useKeyboardShortcut('/', focusSearch);

  function handleToggleCategory(id: string) {
    setExpandedCategories((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  function handleSelectSection(categoryId: string, sectionId: string) {
    setActiveCategory(categoryId);
    setActiveSection(sectionId);
    setSearchQuery('');
    setMobileSidebarOpen(false);
    // Update URL without full navigation
    window.history.replaceState(
      null,
      '',
      `/docs?category=${categoryId}&section=${sectionId}`,
    );
  }

  const renderedContent = useMemo(() => {
    if (!sectionData?.content) return '';
    return renderMarkdown(sectionData.content);
  }, [sectionData]);

  const isSearching = searchQuery.length >= 2;

  if (indexLoading) {
    return <LoadingSpinner className="h-full" message="Loading documentation..." />;
  }

  if (indexError) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 p-8">
        <AlertTriangle size={40} className="text-text-muted" />
        <h2 className="text-lg font-semibold text-text-primary">Documentation Unavailable</h2>
        <p className="max-w-sm text-center text-sm text-text-muted">
          The documentation API is not available. This may be because the docs endpoints
          have not been deployed yet.
        </p>
        <p className="text-xs text-text-muted">
          {indexError instanceof Error ? indexError.message : 'Unknown error'}
        </p>
      </div>
    );
  }

  const categories = docsIndex?.categories ?? [];

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex items-center gap-3 border-b border-border bg-navy-900 px-4 py-2.5">
        <BookOpen size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Documentation</h1>

        {/* Search */}
        <div className="relative ml-4 flex-1 max-w-md">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted" />
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder='Search docs... (press "/" to focus)'
            aria-label="Search documentation"
            className="w-full rounded-md border border-border bg-navy-950 py-1.5 pl-9 pr-3 text-xs text-text-secondary placeholder:text-text-muted focus:border-accent focus:outline-none"
          />
          {searchLoading && (
            <div className="absolute right-3 top-1/2 -translate-y-1/2">
              <LoadingSpinner size={12} />
            </div>
          )}
        </div>

        {/* Mobile sidebar toggle */}
        <button
          onClick={() => setMobileSidebarOpen(!mobileSidebarOpen)}
          className="rounded p-1 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary md:hidden"
          aria-label="Toggle sidebar"
        >
          <FileText size={16} />
        </button>
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <aside
          className={`w-56 flex-shrink-0 overflow-y-auto border-r border-border bg-navy-900 ${
            mobileSidebarOpen ? 'block' : 'hidden'
          } md:block`}
        >
          <DocsSidebar
            categories={categories}
            activeCategory={activeCategory}
            activeSection={activeSection}
            expandedCategories={expandedCategories}
            onToggleCategory={handleToggleCategory}
            onSelectSection={handleSelectSection}
          />
        </aside>

        {/* Main content */}
        <main className="flex-1 overflow-y-auto">
          {isSearching ? (
            searchResults ? (
              <SearchResults
                results={searchResults.results}
                query={searchQuery}
                onSelect={handleSelectSection}
              />
            ) : searchLoading ? (
              <LoadingSpinner className="h-full" message="Searching..." />
            ) : null
          ) : sectionLoading ? (
            <LoadingSpinner className="h-full" message="Loading section..." />
          ) : sectionData ? (
            <div className="mx-auto max-w-3xl px-6 py-6">
              <div className="mb-4">
                <div className="mb-1 flex items-center gap-2">
                  <span className="rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium text-accent">
                    {sectionData.category}
                  </span>
                </div>
                <h2 className="text-xl font-bold text-text-primary">{sectionData.title}</h2>
              </div>
              <article
                className="docs-content prose prose-invert max-w-none text-sm leading-relaxed text-text-secondary
                  [&_h1]:text-lg [&_h1]:font-bold [&_h1]:text-text-primary [&_h1]:mt-8 [&_h1]:mb-3
                  [&_h2]:text-base [&_h2]:font-semibold [&_h2]:text-text-primary [&_h2]:mt-6 [&_h2]:mb-2
                  [&_h3]:text-sm [&_h3]:font-semibold [&_h3]:text-text-primary [&_h3]:mt-4 [&_h3]:mb-2
                  [&_h4]:text-xs [&_h4]:font-semibold [&_h4]:text-text-primary [&_h4]:mt-3 [&_h4]:mb-1
                  [&_p]:mb-3
                  [&_ul]:mb-3 [&_ul]:list-disc [&_ul]:pl-5 [&_ul]:space-y-1
                  [&_ol]:mb-3 [&_ol]:list-decimal [&_ol]:pl-5 [&_ol]:space-y-1
                  [&_li]:text-text-secondary
                  [&_a]:text-accent [&_a]:underline [&_a]:decoration-accent/40 [&_a:hover]:decoration-accent
                  [&_code]:rounded [&_code]:bg-navy-800 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[11px] [&_code]:text-accent
                  [&_pre]:rounded-lg [&_pre]:border [&_pre]:border-border [&_pre]:bg-navy-950 [&_pre]:p-4 [&_pre]:mb-4 [&_pre]:overflow-x-auto
                  [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:text-text-secondary [&_pre_code]:text-[11px] [&_pre_code]:leading-relaxed
                  [&_blockquote]:border-l-2 [&_blockquote]:border-accent/40 [&_blockquote]:pl-4 [&_blockquote]:italic [&_blockquote]:text-text-muted [&_blockquote]:mb-3
                  [&_table]:w-full [&_table]:border-collapse [&_table]:mb-4
                  [&_th]:border [&_th]:border-border [&_th]:bg-navy-900 [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_th]:text-xs [&_th]:font-semibold [&_th]:text-text-primary
                  [&_td]:border [&_td]:border-border [&_td]:px-3 [&_td]:py-2 [&_td]:text-xs
                  [&_hr]:border-border [&_hr]:my-6
                  [&_strong]:font-semibold [&_strong]:text-text-primary
                "
                dangerouslySetInnerHTML={{ __html: renderedContent }}
              />
            </div>
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
              <BookOpen size={40} className="text-text-muted" />
              <h2 className="text-base font-semibold text-text-primary">
                Select a section to get started
              </h2>
              <p className="max-w-sm text-xs text-text-muted">
                Browse the documentation categories in the sidebar, or use the search bar to find
                specific topics.
              </p>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

export default DocsPage;
