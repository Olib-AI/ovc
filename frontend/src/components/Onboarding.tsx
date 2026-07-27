import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Check, ChevronRight, LockKeyhole, ShieldCheck } from 'lucide-react';
import axios from 'axios';
import { useCreateRepo } from '../hooks/useRepo.ts';

function Onboarding() {
  const navigate = useNavigate();
  const createRepo = useCreateRepo();
  const [userName, setUserName] = useState('');
  const [userEmail, setUserEmail] = useState('');
  const [repoName, setRepoName] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  const nameValid = /^[a-zA-Z0-9._-]+$/.test(repoName);
  const passwordsMatch = password === confirmPassword;
  const canSubmit =
    userName.trim().length > 0 &&
    userEmail.trim().length > 0 &&
    nameValid &&
    password.length > 0 &&
    passwordsMatch &&
    !createRepo.isPending;

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    setError(null);
    createRepo.mutate(
      {
        name: repoName.trim(),
        password,
        user_name: userName.trim(),
        user_email: userEmail.trim(),
      },
      {
        onSuccess: (repo) => navigate(`/repo/${repo.id}`),
        onError: (err) => {
          if (axios.isAxiosError(err)) {
            setError(err.message);
          } else {
            setError(err instanceof Error ? err.message : 'Could not finish setup');
          }
        },
      },
    );
  }

  return (
    <div className="h-full overflow-y-auto bg-navy-950 px-4 py-8 sm:px-8">
      <div className="mx-auto grid min-h-full max-w-5xl items-center gap-8 lg:grid-cols-[0.9fr_1.1fr]">
        <section className="px-2 sm:px-6">
          <div className="mb-6 flex h-14 w-14 items-center justify-center rounded-2xl bg-accent/15 ring-1 ring-accent/30">
            <ShieldCheck size={30} className="text-accent" />
          </div>
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent">
            Welcome to OVC
          </p>
          <h1 className="max-w-md text-3xl font-bold tracking-tight text-text-primary sm:text-4xl">
            Your code. Encrypted and under your control.
          </h1>
          <p className="mt-4 max-w-md text-sm leading-6 text-text-secondary">
            Set your commit identity and create your first standalone repository. This information
            is stored inside the encrypted repository, not sent to an external service.
          </p>
          <div className="mt-7 space-y-3 text-sm text-text-secondary">
            {[
              'The desktop app includes its local OVC service',
              'Repository contents are encrypted at rest',
              'You can change identity settings later',
            ].map((item) => (
              <div key={item} className="flex items-center gap-3">
                <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent/15">
                  <Check size={12} className="text-accent" />
                </span>
                {item}
              </div>
            ))}
          </div>
        </section>

        <form
          onSubmit={handleSubmit}
          className="rounded-2xl border border-border bg-navy-900 p-5 shadow-2xl sm:p-7"
        >
          <div className="mb-6">
            <h2 className="text-lg font-semibold text-text-primary">Set up your workspace</h2>
            <p className="mt-1 text-xs text-text-muted">All fields are required for the first repository.</p>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Author name">
              <input
                autoFocus
                value={userName}
                onChange={(event) => setUserName(event.target.value)}
                placeholder="Ada Lovelace"
                autoComplete="name"
                className={inputClass}
              />
            </Field>
            <Field label="Author email">
              <input
                type="email"
                value={userEmail}
                onChange={(event) => setUserEmail(event.target.value)}
                placeholder="ada@example.com"
                autoComplete="email"
                className={inputClass}
              />
            </Field>
          </div>

          <div className="my-5 border-t border-border" />

          <Field label="First repository">
            <input
              value={repoName}
              onChange={(event) => setRepoName(event.target.value)}
              placeholder="my-project"
              spellCheck={false}
              className={inputClass}
            />
          </Field>
          {repoName && !nameValid && (
            <p className="mt-1 text-[11px] text-status-deleted">
              Use letters, numbers, dots, hyphens, or underscores.
            </p>
          )}

          <div className="mt-4 grid gap-4 sm:grid-cols-2">
            <Field label="Encryption password">
              <div className="relative">
                <LockKeyhole
                  size={14}
                  className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-muted"
                />
                <input
                  type="password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  autoComplete="new-password"
                  className={`${inputClass} pl-9`}
                />
              </div>
            </Field>
            <Field label="Confirm password">
              <input
                type="password"
                value={confirmPassword}
                onChange={(event) => setConfirmPassword(event.target.value)}
                autoComplete="new-password"
                className={inputClass}
              />
            </Field>
          </div>
          {confirmPassword && !passwordsMatch && (
            <p className="mt-1 text-[11px] text-status-deleted">Passwords do not match.</p>
          )}

          {error && (
            <div role="alert" className="mt-4 rounded-lg border border-status-deleted/30 bg-status-deleted/10 px-3 py-2 text-xs text-status-deleted">
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={!canSubmit}
            className="mt-6 flex w-full items-center justify-center gap-2 rounded-lg bg-accent px-4 py-2.5 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light disabled:cursor-not-allowed disabled:opacity-40"
          >
            {createRepo.isPending ? 'Creating encrypted repository…' : 'Finish setup'}
            {!createRepo.isPending && <ChevronRight size={16} />}
          </button>
        </form>
      </div>
    </div>
  );
}

const inputClass =
  'w-full rounded-lg border border-border bg-navy-950 px-3 py-2.5 text-sm text-text-primary placeholder-text-muted outline-none transition-colors focus:border-accent focus:ring-1 focus:ring-accent/30';

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs font-medium text-text-secondary">{label}</span>
      {children}
    </label>
  );
}

export default Onboarding;
