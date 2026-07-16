import { useEffect, useState } from "react";
import type { PersistedAccountStatus } from "@/types/account";

const MAX_TIMEOUT_MS = 2_147_483_647;

export function useAccountStatusNow(
  status: PersistedAccountStatus,
  exhaustedUntil: number | null,
): number {
  const [nowMs, setNowMs] = useState(Date.now);

  useEffect(() => {
    if (!isTemporary(status) || exhaustedUntil === null) return;
    const remainingMs = exhaustedUntil - Date.now();
    if (remainingMs <= 0) {
      setNowMs((current) => (current >= exhaustedUntil ? current : Date.now()));
      return;
    }
    const timeout = window.setTimeout(
      () => setNowMs(Date.now()),
      Math.min(remainingMs, MAX_TIMEOUT_MS),
    );
    return () => window.clearTimeout(timeout);
  }, [status, exhaustedUntil, nowMs]);

  return nowMs;
}

function isTemporary(status: PersistedAccountStatus): boolean {
  return status === "quota_exhausted" || status === "cooldown";
}
