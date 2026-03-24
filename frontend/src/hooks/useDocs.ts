import { useQuery } from '@tanstack/react-query';
import * as api from '../api/client.ts';

export function useDocsIndex() {
  return useQuery({
    queryKey: ['docs-index'],
    queryFn: () => api.getDocsIndex(),
    staleTime: 60_000,
  });
}

export function useDocSearch(query: string) {
  return useQuery({
    queryKey: ['docs-search', query],
    queryFn: () => api.searchDocs(query),
    enabled: query.length >= 2,
    staleTime: 30_000,
  });
}

export function useDocSection(category: string | null, section: string | null) {
  return useQuery({
    queryKey: ['docs-section', category, section],
    queryFn: () => api.getDocSection(category!, section!),
    enabled: !!category && !!section,
    staleTime: 60_000,
  });
}
