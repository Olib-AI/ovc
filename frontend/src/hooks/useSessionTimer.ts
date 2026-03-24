import { useState, useEffect, useCallback } from 'react';
import { useAuth } from './useAuth.ts';

/** Decode the `exp` claim from a JWT without verifying the signature. */
function getJwtExpiry(token: string): number | null {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;
    const payload = parts[1];
    // Base64url → base64 → JSON
    const padded = payload.replace(/-/g, '+').replace(/_/g, '/');
    const json = atob(padded);
    const parsed = JSON.parse(json) as { exp?: number };
    return typeof parsed.exp === 'number' ? parsed.exp : null;
  } catch {
    return null;
  }
}

export interface SessionTimerState {
  /** Seconds remaining until token expires. null when unknown. */
  secondsLeft: number | null;
  /** True when < 600 seconds (10 min) remain. */
  isExpiringSoon: boolean;
  /** True when the token has already expired. */
  isExpired: boolean;
}

/**
 * Reads the JWT expiry from the token stored in AuthContext and returns a
 * live countdown. Calls `logout` automatically when the token expires.
 */
export function useSessionTimer(): SessionTimerState {
  const { token, logout } = useAuth();

  const computeSeconds = useCallback((): number | null => {
    if (!token) return null;
    const exp = getJwtExpiry(token);
    if (exp === null) return null;
    return Math.floor(exp - Date.now() / 1000);
  }, [token]);

  const [secondsLeft, setSecondsLeft] = useState<number | null>(computeSeconds);

  useEffect(() => {
    // Recompute immediately whenever the token changes
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setSecondsLeft(computeSeconds());

    const interval = setInterval(() => {
      const secs = computeSeconds();
      setSecondsLeft(secs);
      if (secs !== null && secs <= 0) {
        clearInterval(interval);
        logout();
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [computeSeconds, logout]);

  const isExpired = secondsLeft !== null && secondsLeft <= 0;
  const isExpiringSoon = secondsLeft !== null && secondsLeft > 0 && secondsLeft < 600;

  return { secondsLeft, isExpiringSoon, isExpired };
}

/** Format seconds as "mm:ss" */
export function formatCountdown(seconds: number): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return `${String(m).padStart(2, '0')}:${String(rem).padStart(2, '0')}`;
}
