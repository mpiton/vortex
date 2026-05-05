import { useEffect, useState } from "react";

export interface CountdownState {
  /** Whole seconds remaining until the deadline. Clamped to [0, ∞). */
  remainingSeconds: number;
  /** `MM:SS` (or `HH:MM:SS` past one hour) label, ready to render. */
  label: string;
  /** `true` once `remainingSeconds` hits zero. */
  expired: boolean;
}

/**
 * Live countdown to a Unix-millisecond deadline. Re-renders the host
 * component once per second so the displayed `MM:SS` label stays
 * truthful without each row scheduling its own `setInterval`. Pass
 * `null` when there is no active wait — the hook becomes a no-op and
 * the timer is not scheduled.
 */
export function useCountdown(untilUnixMs: number | null): CountdownState {
  const [now, setNow] = useState<number>(() => Date.now());

  useEffect(() => {
    if (untilUnixMs === null) {
      return;
    }
    setNow(Date.now());
    const interval = setInterval(() => {
      setNow(Date.now());
    }, 1_000);
    return () => clearInterval(interval);
  }, [untilUnixMs]);

  const remainingMs = untilUnixMs === null ? 0 : Math.max(0, untilUnixMs - now);
  const remainingSeconds = Math.ceil(remainingMs / 1_000);
  return {
    remainingSeconds,
    label: formatCountdown(remainingSeconds),
    expired: remainingSeconds === 0,
  };
}

function formatCountdown(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  if (hours > 0) {
    return `${pad(hours)}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(minutes)}:${pad(seconds)}`;
}
