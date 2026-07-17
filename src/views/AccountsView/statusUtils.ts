import type { AccountView, PersistedAccountStatus } from "@/types/account";

export type AccountStatus =
  | "active"
  | "expired"
  | "disabled"
  | "unverified"
  | "invalidCredentials"
  | "missingCredential"
  | "quotaExhausted"
  | "cooldown"
  | "error";

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
  if (account.status === "expired") return "expired";
  if (account.validUntil !== null && account.validUntil < nowMs) return "expired";
  if (
    (account.status === "quota_exhausted" || account.status === "cooldown") &&
    account.exhaustedUntil !== null &&
    nowMs >= account.exhaustedUntil
  ) {
    return "active";
  }

  return persistedAccountStatusToUi(account.status);
}

export function persistedAccountStatusToUi(status: PersistedAccountStatus): AccountStatus {
  switch (status) {
    case "valid":
      return "active";
    case "expired":
      return "expired";
    case "invalid_credentials":
      return "invalidCredentials";
    case "missing_credential":
      return "missingCredential";
    case "quota_exhausted":
      return "quotaExhausted";
    case "cooldown":
      return "cooldown";
    case "error":
      return "error";
    case "unverified":
      return "unverified";
  }
}
