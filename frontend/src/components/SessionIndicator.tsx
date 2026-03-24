import { useSessionTimer } from '../hooks/useSessionTimer.ts';

/**
 * Minimal session status dot.
 * - Green: session healthy
 * - Yellow: expiring soon (< 10 min)
 * - Red: expired / no session
 */
function SessionIndicator() {
  const { secondsLeft, isExpiringSoon } = useSessionTimer();

  const isExpired = secondsLeft !== null && secondsLeft <= 0;
  const isHealthy = secondsLeft === null || (!isExpiringSoon && !isExpired);

  const color = isExpired
    ? 'bg-red-500 shadow-red-500/40'
    : isExpiringSoon
      ? 'bg-yellow-500 shadow-yellow-500/40'
      : 'bg-emerald-500 shadow-emerald-500/40';

  const title = isExpired
    ? 'Session expired'
    : isExpiringSoon
      ? `Session expires soon`
      : 'Session active';

  return (
    <span
      className={`inline-block h-2 w-2 rounded-full shadow-sm ${color} ${!isHealthy ? 'animate-pulse' : ''}`}
      title={title}
      aria-label={title}
    />
  );
}

export default SessionIndicator;
