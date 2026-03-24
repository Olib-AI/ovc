import { useState, useCallback, useMemo, lazy, Suspense } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { AuthContext } from './hooks/useAuth.ts';
import { ThemeProvider } from './contexts/ThemeContext.tsx';
import { ToastProvider } from './contexts/ToastContext.tsx';
import { CommandPaletteProvider } from './contexts/CommandPaletteContext.tsx';
import Layout from './components/Layout.tsx';
import ToastContainer from './components/ToastContainer.tsx';
import LoadingSpinner from './components/LoadingSpinner.tsx';
import LoginPage from './pages/LoginPage.tsx';
import NotFoundPage from './pages/NotFoundPage.tsx';

const RepoListPage = lazy(() => import('./pages/RepoListPage.tsx'));
const RepoPage = lazy(() => import('./pages/RepoPage.tsx'));
const HistoryPage = lazy(() => import('./pages/HistoryPage.tsx'));
const DiffPage = lazy(() => import('./pages/DiffPage.tsx'));
const ActionsPage = lazy(() => import('./pages/ActionsPage.tsx'));
const SettingsPage = lazy(() => import('./pages/SettingsPage.tsx'));
const BlamePage = lazy(() => import('./pages/BlamePage.tsx'));
const SearchPage = lazy(() => import('./pages/SearchPage.tsx'));
const ReflogPage = lazy(() => import('./pages/ReflogPage.tsx'));
const DependenciesPage = lazy(() => import('./pages/DependenciesPage.tsx'));
const PullRequestsPage = lazy(() => import('./pages/PullRequestsPage.tsx'));
const PullRequestPage = lazy(() => import('./pages/PullRequestPage.tsx'));
const DocsPage = lazy(() => import('./pages/DocsPage.tsx'));
const AccessPage = lazy(() => import('./pages/AccessPage.tsx'));
const RepoOverviewPage = lazy(() => import('./pages/RepoOverviewPage.tsx'));

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 10_000,
      gcTime: 60_000, // garbage-collect unused query data after 60s (default was 5min)
      refetchOnWindowFocus: false,
    },
  },
});

function App() {
  const [token, setToken] = useState<string | null>(() => localStorage.getItem('ovc_token'));

  const login = useCallback((newToken: string) => {
    localStorage.setItem('ovc_token', newToken);
    setToken(newToken);
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem('ovc_token');
    queryClient.clear();
    setToken(null);
  }, []);

  const authValue = useMemo(
    () => ({
      token,
      login,
      logout,
      isAuthenticated: !!token,
    }),
    [token, login, logout],
  );

  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <AuthContext.Provider value={authValue}>
          <ToastProvider>
            <CommandPaletteProvider>
              <BrowserRouter>
                <Suspense fallback={<LoadingSpinner className="h-screen" message="Loading..." />}>
                  <Routes>
                    <Route path="/login" element={<LoginPage />} />
                    <Route
                      element={token ? <Layout /> : <Navigate to="/login" replace />}
                    >
                      <Route path="/" element={<RepoListPage />} />
                      <Route path="/repo/:repoId/overview" element={<RepoOverviewPage />} />
                      <Route path="/repo/:repoId" element={<RepoPage />} />
                      <Route path="/repo/:repoId/actions" element={<ActionsPage />} />
                      <Route path="/repo/:repoId/history" element={<HistoryPage />} />
                      <Route path="/repo/:repoId/diff" element={<DiffPage />} />
                      <Route path="/repo/:repoId/settings" element={<SettingsPage />} />
                      <Route path="/repo/:repoId/blame/*" element={<BlamePage />} />
                      <Route path="/repo/:repoId/search" element={<SearchPage />} />
                      <Route path="/repo/:repoId/reflog" element={<ReflogPage />} />
                      <Route path="/repo/:repoId/dependencies" element={<DependenciesPage />} />
                      <Route path="/repo/:repoId/access" element={<AccessPage />} />
                      <Route path="/repo/:repoId/pulls" element={<PullRequestsPage />} />
                      <Route path="/repo/:repoId/pulls/*" element={<PullRequestPage />} />
                      <Route path="/docs" element={<DocsPage />} />
                    </Route>
                    <Route path="*" element={token ? <NotFoundPage /> : <Navigate to="/login" replace />} />
                  </Routes>
                </Suspense>
              </BrowserRouter>
              <ToastContainer />
            </CommandPaletteProvider>
          </ToastProvider>
        </AuthContext.Provider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}

export default App;
