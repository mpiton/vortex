import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { usePluginStore } from "../usePluginStore";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

function wrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return createElement(QueryClientProvider, { client: qc }, children);
}

const mockEntries = [
  {
    name: "vortex-mod-youtube",
    description: "YouTube downloader",
    author: "vortex-team",
    version: "1.0.0",
    installedVersion: "1.0.0",
    category: "crawler",
    official: true,
    status: "installed" as const,
  },
  {
    name: "vortex-mod-gallery",
    description: "Gallery hoster",
    author: "johndoe",
    version: "1.1.0",
    installedVersion: "1.0.0",
    category: "hoster",
    official: false,
    status: "update_available" as const,
  },
];

describe("usePluginStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("should return store entries from plugin_store_list", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockEntries);
    const { result } = renderHook(() => usePluginStore(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.entries).toHaveLength(2);
    expect(result.current.entries[0].name).toBe("vortex-mod-youtube");
  });

  it("should expose installPlugin mutation", () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const { result } = renderHook(() => usePluginStore(), { wrapper });
    expect(typeof result.current.installPlugin).toBe("function");
  });

  it("should expose updatePlugin mutation", () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const { result } = renderHook(() => usePluginStore(), { wrapper });
    expect(typeof result.current.updatePlugin).toBe("function");
  });

  it("should expose refreshStore function", () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const { result } = renderHook(() => usePluginStore(), { wrapper });
    expect(typeof result.current.refreshStore).toBe("function");
  });
});
