import { useEffect, useState } from "react";
import type { PersistedAccountStatus } from "@/types/account";

const MAX_TIMEOUT_MS = 2_147_483_647;

export function useAccountStatusNow(
  status: PersistedAccountStatus,
  exhaustedUntil: number | null,
  validUntil: number | null,
): number {
  const [nowMs, setNowMs] = useState(Date.now);

  useEffect(() => {
    const currentTime = Date.now();
    const deadlines = statusDeadlines(status, exhaustedUntil, validUntil);
    if (deadlines.some((deadline) => nowMs < deadline && deadline <= currentTime)) {
      setNowMs(currentTime);
      return;
    }
    const nextDeadline =
      deadlines.filter((deadline) => deadline > currentTime).sort((a, b) => a - b)[0] ?? null;
    if (nextDeadline === null) return;
    const timeout = window.setTimeout(
      () => setNowMs(Date.now()),
      Math.min(nextDeadline - currentTime, MAX_TIMEOUT_MS),
    );
    return () => window.clearTimeout(timeout);
  }, [status, exhaustedUntil, validUntil, nowMs]);

  return nowMs;
}

function statusDeadlines(
  status: PersistedAccountStatus,
  exhaustedUntil: number | null,
  validUntil: number | null,
): number[] {
  return [
    isTemporary(status) ? exhaustedUntil : null,
    validUntil === null ? null : validUntil + 1,
  ].filter((deadline): deadline is number => deadline !== null);
}

function isTemporary(status: PersistedAccountStatus): boolean {
  return status === "quota_exhausted" || status === "cooldown";
}
