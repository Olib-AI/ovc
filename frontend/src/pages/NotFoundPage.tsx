import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';

function NotFoundPage() {
  useDocumentTitle('Not Found \u2014 OVC');

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 text-center">
      <h1 className="text-5xl font-bold text-text-muted/30">404</h1>
      <h2 className="text-lg font-semibold text-text-primary">Page not found</h2>
      <p className="max-w-sm text-sm text-text-muted">
        The page you&apos;re looking for doesn&apos;t exist or has been moved.
      </p>
      <a
        href="/"
        className="mt-2 rounded bg-accent px-4 py-2 text-sm font-semibold text-navy-950 transition-colors hover:bg-accent-light"
      >
        Back to home
      </a>
    </div>
  );
}

export default NotFoundPage;
