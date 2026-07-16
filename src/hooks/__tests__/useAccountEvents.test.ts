import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { useAccountEvents } from "@/hooks/useAccountEvents";

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

vi.mock("@/api/client", () => ({
  queryClient: {
    invalidateQueries: vi.fn(),
  },
}));

import { queryClient } from "@/api/client";
import { useTauriEvent } from "@/hooks/useTauriEvent";

describe("useAccountEvents", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("subscribes to every event that changes account availability", () => {
    renderHook(() => useAccountEvents());

    expect(vi.mocked(useTauriEvent).mock.calls.map(([event]) => event)).toEqual([
      "account-added",
      "account-updated",
      "account-deleted",
      "account-validated",
      "account-validation-failed",
      "account-exhausted",
    ]);
  });

  it("invalidates account queries when automatic validation fails", () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === "account-validation-failed") {
        callback({ id: "account-1", error: "expired" });
      }
    });

    renderHook(() => useAccountEvents());

    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["accounts"],
    });
  });
});
