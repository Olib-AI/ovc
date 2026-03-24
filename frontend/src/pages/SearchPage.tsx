import { useState, useCallback, useMemo } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { useQuery } from '@tanstack/react-query';
import { searchCode } from '../api/client.ts';
import type { SearchMatch } from '../api/types.ts';
import SearchBar from '../components/SearchBar.tsx';
import LoadingSpinner from '../components/LoadingSpinner.tsx';
import { Search, FileCode, ChevronDown, ChevronRight, AlertTriangle } from 'lucide-react';

interface GroupedResults {
  path: string;
  matches: SearchMatch[];
}

/** A segment of a line to render — either plain text or a highlighted match. */
interface LineSegment {
  text: string;
  highlight: boolean;
}

/**
 * Splits `line` into plain/highlighted segments by finding all occurrences of
 * `pattern` (already compiled into a RegExp with the correct flags by the caller).
 * Returns a single plain segment when the pattern produces no matches.
 */
function buildLineSegments(line: string, pattern: RegExp): LineSegment[] {
  const segments: LineSegment[] = [];
  let lastIndex = 0;

  for (const match of line.matchAll(pattern)) {
    const start = match.index;
    const end = start + match[0].length;

    if (start > lastIndex) {
      segments.push({ text: line.slice(lastIndex, start), highlight: false });
    }
    segments.push({ text: match[0], highlight: true });
    lastIndex = end;
  }

  if (lastIndex < line.length) {
    segments.push({ text: line.slice(lastIndex), highlight: false });
  }

  return segments.length > 0 ? segments : [{ text: line, highlight: false }];
}

function SearchPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Search \u2014 OVC`);
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useState<{
    query: string;
    caseInsensitive: boolean;
    filePattern?: string;
    isRegex: boolean;
  } | null>(null);

  const { data: results, isLoading, error } = useQuery({
    queryKey: [
      'repo', repoId, 'search',
      searchParams?.query,
      searchParams?.caseInsensitive,
      searchParams?.filePattern,
      searchParams?.isRegex,
    ],
    queryFn: () =>
      searchCode(
        repoId!,
        searchParams!.query,
        searchParams!.caseInsensitive,
        searchParams!.filePattern,
        searchParams!.isRegex,
      ),
    enabled: !!repoId && !!searchParams?.query,
    gcTime: 10_000, // search results can be large — GC quickly after unmount
  });

  const handleSearch = useCallback(
    (query: string, caseInsensitive: boolean, filePattern?: string, isRegex = false) => {
      setSearchParams({ query, caseInsensitive, filePattern, isRegex });
    },
    [],
  );

  /**
   * Build the highlight pattern once per query/caseInsensitive change so that
   * all match rows can share the same RegExp instance. Using `matchAll` requires
   * the `g` flag. We escape the raw query so special regex characters are treated
   * as literals — the backend performs a literal substring/regex search depending
   * on its own logic, but highlighting is always literal-match for safety.
   */
  const highlightPattern = useMemo((): RegExp | null => {
    if (!searchParams?.query) return null;
    const flags = searchParams.caseInsensitive ? 'gi' : 'g';
    // When regex mode is active, use the raw query as the pattern.
    // Wrap in a try/catch so an invalid regex doesn't crash the render.
    if (searchParams.isRegex) {
      try {
        return new RegExp(searchParams.query, flags);
      } catch {
        return null;
      }
    }
    const escaped = searchParams.query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return new RegExp(escaped, flags);
  }, [searchParams]);

  const groupedResults = useMemo((): GroupedResults[] => {
    if (!results) return [];
    const groups = new Map<string, SearchMatch[]>();
    for (const match of results.results) {
      const existing = groups.get(match.path);
      if (existing) {
        existing.push(match);
      } else {
        groups.set(match.path, [match]);
      }
    }
    return Array.from(groups.entries()).map(([path, matches]) => ({ path, matches }));
  }, [results]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Search size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Code Search</h1>
        {results && (
          <span className="text-xs text-text-muted">
            {results.total_matches} match{results.total_matches !== 1 ? 'es' : ''}
          </span>
        )}
        {results?.truncated && (
          <span className="ml-1 flex items-center gap-1 rounded bg-yellow-500/15 px-2 py-0.5 text-[11px] text-yellow-400">
            <AlertTriangle size={11} />
            Results truncated
          </span>
        )}
      </div>

      <div className="border-b border-border bg-navy-900/50 px-4 py-3">
        <SearchBar onSearch={handleSearch} placeholder="Search across all files..." />
      </div>

      <div className="flex-1 overflow-y-auto">
        {isLoading && <LoadingSpinner className="py-12" message="Searching..." />}

        {error && (
          <div className="p-8 text-sm text-status-deleted">
            Search failed: {(error as Error).message}
          </div>
        )}

        {!isLoading && !error && results && results.results.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-text-muted">
            <Search size={40} className="mb-3 opacity-30" />
            <p className="text-sm">No results found for "{results.query}"</p>
          </div>
        )}

        {!isLoading && !error && !results && !searchParams && (
          <div className="flex flex-col items-center justify-center py-16 text-text-muted">
            <Search size={40} className="mb-3 opacity-30" />
            <p className="text-sm">Enter a query to search across all files</p>
          </div>
        )}

        {groupedResults.length > 0 && (
          <div className="divide-y divide-border">
            {groupedResults.map((group) => (
              <FileResultGroup
                key={group.path}
                group={group}
                highlightPattern={highlightPattern}
                onClickMatch={(match) => navigate(`/repo/${repoId}?file=${encodeURIComponent(match.path)}&line=${match.line_number}`)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

interface FileResultGroupProps {
  group: GroupedResults;
  highlightPattern: RegExp | null;
  onClickMatch: (match: SearchMatch) => void;
}

function FileResultGroup({ group, highlightPattern, onClickMatch }: FileResultGroupProps) {
  const [expanded, setExpanded] = useState(true);

  return (
    <div>
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-2 bg-navy-800/50 px-4 py-2 text-left transition-colors hover:bg-navy-800"
      >
        {expanded ? (
          <ChevronDown size={14} className="text-text-muted" />
        ) : (
          <ChevronRight size={14} className="text-text-muted" />
        )}
        <FileCode size={14} className="text-accent" />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-text-primary">
          {group.path}
        </span>
        <span className="text-[11px] text-text-muted">
          {group.matches.length} match{group.matches.length !== 1 ? 'es' : ''}
        </span>
      </button>

      {expanded && (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse font-mono text-[13px] leading-5">
            <tbody>
              {group.matches.map((match, idx) => (
                <tr
                  key={idx}
                  onClick={() => onClickMatch(match)}
                  className="cursor-pointer transition-colors hover:bg-surface-hover/50"
                >
                  <td className="w-12 select-none border-r border-border/20 px-3 text-right text-text-muted/50">
                    {match.line_number}
                  </td>
                  <td className="whitespace-pre px-4 text-text-secondary">
                    <HighlightedLine line={match.line || '\u00A0'} pattern={highlightPattern} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

interface HighlightedLineProps {
  line: string;
  pattern: RegExp | null;
}

function HighlightedLine({ line, pattern }: HighlightedLineProps) {
  if (!pattern) {
    return <>{line}</>;
  }

  // Create a fresh RegExp per render so we never mutate the shared prop instance.
  // matchAll requires the `g` flag, which tracks lastIndex internally — owning a
  // local copy guarantees correct iteration regardless of render order.
  const localPattern = new RegExp(pattern.source, pattern.flags);
  const segments = buildLineSegments(line, localPattern);

  return (
    <>
      {segments.map((seg, i) =>
        seg.highlight ? (
          <mark
            key={i}
            className="rounded-sm bg-yellow-400/30 text-inherit"
          >
            {seg.text}
          </mark>
        ) : (
          <span key={i}>{seg.text}</span>
        ),
      )}
    </>
  );
}

export default SearchPage;
