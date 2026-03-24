import { Loader2 } from 'lucide-react';

interface LoadingSpinnerProps {
  size?: number;
  className?: string;
  message?: string;
}

function LoadingSpinner({ size = 24, className = '', message }: LoadingSpinnerProps) {
  return (
    <div className={`flex items-center justify-center gap-3 ${className}`}>
      <Loader2 size={size} className="animate-spin text-accent" />
      {message && <span className="text-text-secondary text-sm">{message}</span>}
    </div>
  );
}

export default LoadingSpinner;
