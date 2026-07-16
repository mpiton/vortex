import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountView } from "@/types/account";
import { AccountRow, type AccountRowActions } from "../AccountRow";

const actions: AccountRowActions = {
  validate: vi.fn(),
  edit: vi.fn(),
  delete: vi.fn(),
  toggleEnabled: vi.fn(),
};

function renderRow(account: AccountView) {
  render(
    <table>
      <tbody>
        <AccountRow account={account} actions={actions} />
      </tbody>
    </table>,
  );
}

afterEach(() => {
  vi.useRealTimers();
});

describe("AccountRow", () => {
  it("wakes at the cooldown deadline without requiring a backend event", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    renderRow({
      id: "account-1",
      serviceName: "vortex-mod-1fichier",
      username: "alice",
      accountType: "premium",
      enabled: true,
      trafficLeft: null,
      trafficTotal: null,
      validUntil: null,
      lastValidated: null,
      createdAt: 1_699_999_000_000,
      status: "cooldown",
      exhaustedUntil: 1_700_000_001_000,
    });

    expect(screen.getByText("Rate limited")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(1_000));
    expect(screen.getByText("Active")).toBeInTheDocument();
  });
});
