import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { tauriInvoke, queryClient } from "@/api/client";

describe("tauriInvoke", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return typed data on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "1", fileName: "test.zip" });
    const result = await tauriInvoke<{ id: string; fileName: string }>("get_download", { id: "1" });
    expect(result).toEqual({ id: "1", fileName: "test.zip" });
    expect(invoke).toHaveBeenCalledWith("get_download", { id: "1" });
  });

  it("should propagate errors thrown by invoke", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("command not found"));
    await expect(tauriInvoke("unknown_command")).rejects.toThrow("command not found");
  });

  it("should call invoke without args when none provided", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await tauriInvoke("list_downloads");
    expect(invoke).toHaveBeenCalledWith("list_downloads", undefined);
  });
});

describe("queryClient", () => {
  it("should have staleTime of 5 minutes", () => {
    const defaults = queryClient.getDefaultOptions();
    expect(defaults.queries?.staleTime).toBe(5 * 60 * 1000);
  });

  it("should have gcTime of 10 minutes", () => {
    const defaults = queryClient.getDefaultOptions();
    expect(defaults.queries?.gcTime).toBe(10 * 60 * 1000);
  });

  it("should retry once", () => {
    const defaults = queryClient.getDefaultOptions();
    expect(defaults.queries?.retry).toBe(1);
  });

  it("should not refetch on window focus", () => {
    const defaults = queryClient.getDefaultOptions();
    expect(defaults.queries?.refetchOnWindowFocus).toBe(false);
  });
});
