import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import { GitBranch, Lock, Sun, Moon } from 'lucide-react';
import { getToken } from '../api/client.ts';
import { useAuth } from '../hooks/useAuth.ts';
import { useTheme } from '../contexts/ThemeContext.tsx';

function LoginPage() {
  useDocumentTitle('Login \u2014 OVC');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const { login } = useAuth();
  const navigate = useNavigate();
  const { theme, toggleTheme } = useTheme();

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!password) return;

    setLoading(true);
    setError(null);

    try {
      const response = await getToken(password);
      login(response.token);
      navigate('/');
    } catch (err: unknown) {
      // Surface rate-limit errors distinctly so users know to wait.
      if (
        err &&
        typeof err === 'object' &&
        'response' in err &&
        (err as { response?: { status?: number } }).response?.status === 429
      ) {
        setError('Too many login attempts. Please wait a minute and try again.');
      } else {
        setError('Authentication failed. Check your password.');
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-navy-950 p-4">
      <button
        onClick={toggleTheme}
        className="fixed top-4 right-4 rounded-md border border-border bg-navy-900 p-2 text-text-muted transition-colors hover:bg-surface-hover hover:text-text-primary"
        title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
        aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
      >
        {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
      </button>
      <div className="w-full max-w-sm">
        <div className="mb-8 text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-xl bg-accent/15">
            <GitBranch size={28} className="text-accent" />
          </div>
          <h1 className="text-2xl font-bold text-text-primary">OVC</h1>
          <p className="mt-1 text-sm text-text-muted">Olib Version Control</p>
          <a
            href="https://www.olib.ai"
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-accent/70 transition-colors hover:text-accent"
          >
            www.olib.ai
          </a>
        </div>

        <form
          onSubmit={handleSubmit}
          className="rounded-lg border border-border bg-navy-900 p-6 shadow-lg"
        >
          <div className="mb-4 flex items-center gap-2 text-sm text-text-secondary">
            <Lock size={16} className="text-accent" />
            <span>Enter your API password</span>
          </div>

          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Password"
            aria-label="API password"
            className="w-full rounded border border-border bg-navy-950 px-3 py-2.5 text-sm text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            autoFocus
          />

          {error && (
            <p className="mt-2 text-xs text-status-deleted">{error}</p>
          )}

          <button
            type="submit"
            disabled={!password || loading}
            className="mt-4 w-full rounded bg-accent py-2.5 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:opacity-50"
          >
            {loading ? 'Authenticating...' : 'Sign In'}
          </button>
        </form>

        <p className="mt-6 text-center text-[11px] text-text-muted">
          Encrypted version control for your data
        </p>
      </div>
    </div>
  );
}

export default LoginPage;
