import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";

import {
  invoke,
  listen,
  emitMockEvent,
  clearMockListeners,
} from "@/test/__mocks__/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { useClipboardMonitoring } from "../useClipboardMonitoring";

function wrapper({ children }: { children: React.ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return createElement(QueryClientProvider, { client }, children);
}

describe("useClipboardMonitoring", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearMockListeners();
  });

  it("should return initial enabled state", () => {
    const { result } = renderHook(() => useClipboardMonitoring(true), {
      wrapper,
    });
    expect(result.current.isEnabled).toBe(true);
  });

  it("should return initial disabled state", () => {
    const { result } = renderHook(() => useClipboardMonitoring(false), {
      wrapper,
    });
    expect(result.current.isEnabled).toBe(false);
  });

  it("should update state on toggle call", async () => {
    invoke.mockResolvedValue(true);
    const { result } = renderHook(() => useClipboardMonitoring(false), {
      wrapper,
    });

    await act(async () => {
      result.current.toggle(true);
    });

    expect(result.current.isEnabled).toBe(true);
    expect(invoke).toHaveBeenCalledWith("clipboard_toggle", {
      enabled: true,
    });
  });

  it("should update state on clipboard-monitoring-changed event", async () => {
    const { result } = renderHook(() => useClipboardMonitoring(false), {
      wrapper,
    });

    act(() => {
      emitMockEvent("clipboard-monitoring-changed", { enabled: true });
    });

    expect(result.current.isEnabled).toBe(true);
  });
});
