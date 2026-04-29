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
    credentialRef: "keyring://real-debrid/alice",
    ...overrides,
  };
}

describe("deriveAccountStatus", () => {
  it("returns 'disabled' when the account is disabled even if otherwise valid", () => {
    const account = base({
      enabled: false,
      lastValidated: 1_000,
      validUntil: 2_000_000_000_000,
    });
    expect(deriveAccountStatus(account, 1)).toBe("disabled");
  });

  it("returns 'expired' when valid_until is in the past", () => {
    const account = base({ validUntil: 1, lastValidated: 0 });
    expect(deriveAccountStatus(account, 100)).toBe("expired");
  });

  it("returns 'unverified' when lastValidated is null", () => {
    const account = base({ lastValidated: null, validUntil: 100_000 });
    expect(deriveAccountStatus(account, 1)).toBe("unverified");
  });

  it("returns 'active' when enabled, validated, not expired", () => {
    const account = base({ lastValidated: 1, validUntil: 100_000 });
    expect(deriveAccountStatus(account, 1)).toBe("active");
  });

  it("returns 'active' when validUntil is null but lastValidated set", () => {
    const account = base({ lastValidated: 1, validUntil: null });
    expect(deriveAccountStatus(account, 1)).toBe("active");
  });
});
