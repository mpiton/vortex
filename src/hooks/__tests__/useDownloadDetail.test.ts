import { describe, it, expect, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { useDownloadDetail } from "../useDownloadDetail";
import type { DownloadDetailView } from "@/types/download";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

function mockDownloadDetail(): DownloadDetailView {
  return {
    id: "dl-1",
    fileName: "test-file.zip",
    url: "https://example.com/test-file.zip",
    sourceHostname: "example.com",
    state: "Downloading",
    progressPercent: 50,
    speedBytesPerSec: 1048576,
    downloadedBytes: 524288,
    totalBytes: 1048576,
    etaSeconds: 30,
    segments: [],
    checksumExpected: null,
    checksumComputed: null,
    checksumAlgorithm: null,
    destinationPath: "/home/user/Downloads/test-file.zip",
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 5,
    mirrors: [],
    currentMirrorIndex: 0,
    createdAt: Date.now(),
    updatedAt: Date.now(),
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return ({ children }: { children: React.ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

describe("useDownloadDetail", () => {
  it("should call download_detail with numeric id", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(mockDownloadDetail());

    const { result } = renderHook(() => useDownloadDetail("7274895108243456"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("download_detail", { id: 7274895108243456 });
    expect(result.current.data?.id).toBe("dl-1");
  });
});
