import type { AccountView } from "@/types/account";

export type AccountStatus = "active" | "expired" | "disabled" | "unverified";

/**
 * Derives a UI status badge for an account row from its persisted state.
 * Order matters: a disabled account is always shown as "disabled" even if
 * its `valid_until` is still in the future, so users see the same label
 * the toggle just produced.
 */
export function deriveAccountStatus(
  account: AccountView,
  nowMs: number = Date.now(),
): AccountStatus {
  if (!account.enabled) return "disabled";
  if (account.validUntil !== null && account.validUntil < nowMs) return "expired";
  if (account.lastValidated === null) return "unverified";
  return "active";
}
