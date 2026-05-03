import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { ReactNode } from "react";
import { createElement } from "react";
import { useHistoryQuery } from "../useHistoryQuery";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

function wrapper(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function freshClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
}

describe("useHistoryQuery", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should invoke history_list when search is empty", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const { result } = renderHook(() => useHistoryQuery(), {
      wrapper: wrapper(freshClient()),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(mockInvoke).toHaveBeenCalledWith("history_list", undefined);
  });

  it("should invoke history_search with trimmed query when search is set", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const { result } = renderHook(() => useHistoryQuery({ searchQuery: "  movie  " }), {
      wrapper: wrapper(freshClient()),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(mockInvoke).toHaveBeenCalledWith("history_search", { q: "movie" });
  });

  it("should fall back to history_list when search is whitespace only", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const { result } = renderHook(() => useHistoryQuery({ searchQuery: "   " }), {
      wrapper: wrapper(freshClient()),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(mockInvoke).toHaveBeenCalledWith("history_list", undefined);
  });

  it("should propagate errors from the IPC layer", async () => {
    mockInvoke.mockRejectedValueOnce("backend down");
    const { result } = renderHook(() => useHistoryQuery(), {
      wrapper: wrapper(freshClient()),
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toContain("backend down");
  });
});
