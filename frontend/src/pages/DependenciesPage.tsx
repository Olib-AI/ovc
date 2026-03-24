import { useParams } from 'react-router-dom';
import { Package } from 'lucide-react';
import { useDocumentTitle } from '../hooks/useDocumentTitle.ts';
import DependencyDashboard from '../components/DependencyDashboard.tsx';

function DependenciesPage() {
  const { repoId } = useParams<{ repoId: string }>();
  useDocumentTitle(`${repoId ?? 'Repo'} \u2014 Dependencies \u2014 OVC`);

  if (!repoId) return null;

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex items-center gap-2 border-b border-border bg-navy-900 px-4 py-2.5">
        <Package size={16} className="text-accent" />
        <h1 className="text-sm font-semibold text-text-primary">Dependencies</h1>
      </div>

      <div className="mx-auto max-w-3xl p-6">
        <DependencyDashboard repoId={repoId} />
      </div>
    </div>
  );
}

export default DependenciesPage;
