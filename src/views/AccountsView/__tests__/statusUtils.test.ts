import { describe, it, expect } from "vitest";
import type { AccountView } from "@/types/account";
import { deriveAccountStatus } from "../statusUtils";

function base(overrides: Partial<AccountView> = {}): AccountView {
  return {
    id: "id",
    serviceName: "real-debrid",
    username: "alice",
    accountType: "premium",
    enabled: true,
    trafficLeft: null,
    trafficTotal: null,
    validUntil: null,
    lastValidated: null,
    createdAt: 0,
    status: "unverified",
    exhaustedUntil: null,
    ...overrides,
  };
}

describe("deriveAccountStatus", () => {
  it("returns 'disabled' when the account is disabled even if otherwise valid", () => {
    const account = base({
      enabled: false,
      status: "valid",
      lastValidated: 1_000,
      validUntil: 2_000_000_000_000,
    });
    expect(deriveAccountStatus(account, 1)).toBe("disabled");
  });

  it("returns 'expired' when valid_until is in the past", () => {
    const account = base({ status: "valid", validUntil: 1, lastValidated: 0 });
    expect(deriveAccountStatus(account, 100)).toBe("expired");
  });

  it("returns 'unverified' when lastValidated is null", () => {
    const account = base({ lastValidated: null, validUntil: 100_000 });
    expect(deriveAccountStatus(account, 1)).toBe("unverified");
  });

  it("returns 'active' when enabled, validated, not expired", () => {
    const account = base({ status: "valid", lastValidated: 1, validUntil: 100_000 });
    expect(deriveAccountStatus(account, 1)).toBe("active");
  });

  it("returns 'active' when validUntil is null but lastValidated set", () => {
    const account = base({ status: "valid", lastValidated: 1, validUntil: null });
    expect(deriveAccountStatus(account, 1)).toBe("active");
  });

  it.each(["quota_exhausted", "cooldown"] as const)(
    "returns 'active' when temporary status '%s' has elapsed",
    (status) => {
      expect(deriveAccountStatus(base({ status, exhaustedUntil: 100 }), 100)).toBe("active");
    },
  );

  it.each([
    ["quota_exhausted", "quotaExhausted"],
    ["cooldown", "cooldown"],
  ] as const)("keeps temporary status '%s' until its deadline", (status, expected) => {
    expect(deriveAccountStatus(base({ status, exhaustedUntil: 101 }), 100)).toBe(expected);
  });

  it.each([
    ["invalid_credentials", "invalidCredentials"],
    ["missing_credential", "missingCredential"],
    ["expired", "expired"],
    ["quota_exhausted", "quotaExhausted"],
    ["cooldown", "cooldown"],
    ["error", "error"],
  ] as const)("maps persisted '%s' to '%s'", (persisted, expected) => {
    expect(deriveAccountStatus(base({ status: persisted }), 1)).toBe(expected);
  });
});
