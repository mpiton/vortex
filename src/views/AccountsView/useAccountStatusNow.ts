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
    const nextDeadline = nextStatusDeadline(status, exhaustedUntil, validUntil, currentTime);
    if (nextDeadline === null) return;
    const timeout = window.setTimeout(
      () => setNowMs(Date.now()),
      Math.min(nextDeadline - currentTime, MAX_TIMEOUT_MS),
    );
    return () => window.clearTimeout(timeout);
  }, [status, exhaustedUntil, validUntil, nowMs]);

  return nowMs;
}

function nextStatusDeadline(
  status: PersistedAccountStatus,
  exhaustedUntil: number | null,
  validUntil: number | null,
  nowMs: number,
): number | null {
  const deadlines = [
    isTemporary(status) ? exhaustedUntil : null,
    validUntil === null ? null : validUntil + 1,
  ].filter((deadline): deadline is number => deadline !== null && deadline > nowMs);
  return deadlines.length === 0 ? null : Math.min(...deadlines);
}

function isTemporary(status: PersistedAccountStatus): boolean {
  return status === "quota_exhausted" || status === "cooldown";
}
