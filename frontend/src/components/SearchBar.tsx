import { useState, useEffect, useRef } from 'react';
import { Search, CaseSensitive, Filter } from 'lucide-react';

interface SearchBarProps {
  onSearch: (query: string, caseInsensitive: boolean, filePattern?: string, isRegex?: boolean) => void;
  initialQuery?: string;
  placeholder?: string;
}

function SearchBar({ onSearch, initialQuery = '', placeholder = 'Search code...' }: SearchBarProps) {
  const [query, setQuery] = useState(initialQuery);
  const [caseInsensitive, setCaseInsensitive] = useState(false);
  const [isRegex, setIsRegex] = useState(false);
  const [filePattern, setFilePattern] = useState('');
  const [showFilter, setShowFilter] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    if (!query.trim()) return;

    debounceRef.current = setTimeout(() => {
      onSearch(query.trim(), caseInsensitive, filePattern.trim() || undefined, isRegex);
    }, 300);

    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [query, caseInsensitive, filePattern, isRegex, onSearch]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }
    if (query.trim()) {
      onSearch(query.trim(), caseInsensitive, filePattern.trim() || undefined, isRegex);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <div className="flex flex-1 items-center gap-2 rounded border border-border bg-navy-950 px-3 py-2">
          <Search size={14} className="flex-shrink-0 text-text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={placeholder}
            aria-label="Search query"
            className="flex-1 bg-transparent text-sm text-text-primary placeholder-text-muted focus:outline-none"
          />
        </div>
        <button
          type="button"
          onClick={() => setShowFilter((v) => !v)}
          className={`rounded border px-2.5 py-2 text-xs font-medium transition-colors ${
            showFilter || filePattern
              ? 'border-accent bg-accent/15 text-accent'
              : 'border-border text-text-muted hover:border-text-muted hover:text-text-secondary'
          }`}
          title="File pattern filter"
          aria-label="Toggle file pattern filter"
        >
          <Filter size={14} />
        </button>
        <button
          type="button"
          onClick={() => setCaseInsensitive((prev) => !prev)}
          className={`rounded border px-2.5 py-2 text-xs font-medium transition-colors ${
            caseInsensitive
              ? 'border-accent bg-accent/15 text-accent'
              : 'border-border text-text-muted hover:border-text-muted hover:text-text-secondary'
          }`}
          title={caseInsensitive ? 'Case insensitive (on)' : 'Case sensitive'}
          aria-label="Toggle case sensitivity"
        >
          <CaseSensitive size={14} />
        </button>
        <button
          type="button"
          onClick={() => setIsRegex((prev) => !prev)}
          className={`rounded border px-2.5 py-2 font-mono text-xs font-medium transition-colors ${
            isRegex
              ? 'border-accent bg-accent/15 text-accent'
              : 'border-border text-text-muted hover:border-text-muted hover:text-text-secondary'
          }`}
          title={isRegex ? 'Regex mode (on)' : 'Use regular expression'}
          aria-label="Toggle regex mode"
          aria-pressed={isRegex}
        >
          .*
        </button>
        <button
          type="submit"
          className="rounded bg-accent/15 px-4 py-2 text-sm font-medium text-accent transition-colors hover:bg-accent/25"
        >
          Search
        </button>
      </div>
      {showFilter && (
        <div className="flex items-center gap-2 rounded border border-border bg-navy-950 px-3 py-2">
          <Filter size={14} className="flex-shrink-0 text-text-muted" />
          <input
            value={filePattern}
            onChange={(e) => setFilePattern(e.target.value)}
            placeholder="File pattern (e.g. *.py, src/**/*.ts)"
            aria-label="File pattern filter"
            className="flex-1 bg-transparent text-sm text-text-primary placeholder-text-muted focus:outline-none"
          />
          {filePattern && (
            <button
              type="button"
              onClick={() => setFilePattern('')}
              className="text-text-muted transition-colors hover:text-text-primary"
              aria-label="Clear file pattern"
            >
              &times;
            </button>
          )}
        </div>
      )}
    </form>
  );
}

export default SearchBar;
