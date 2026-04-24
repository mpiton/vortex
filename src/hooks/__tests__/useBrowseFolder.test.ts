import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useBrowseFolder, useBrowseFile } from "../useBrowseFolder";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

async function getInvoke() {
  const mod = await import("@tauri-apps/api/core");
  return vi.mocked(mod.invoke);
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("useBrowseFolder", () => {
  it("should return null when the user cancels the dialog", async () => {
    const invoke = await getInvoke();
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useBrowseFolder());
    let selected: string | null = "not-called";
    await act(async () => {
      selected = await result.current();
    });

    expect(selected).toBeNull();
    expect(invoke).toHaveBeenCalledWith("browse_folder", { defaultPath: null });
  });

  it("should forward the default path and return the picked folder", async () => {
    const invoke = await getInvoke();
    invoke.mockResolvedValue("/home/me/Pictures");

    const { result } = renderHook(() => useBrowseFolder());
    let selected: string | null = null;
    await act(async () => {
      selected = await result.current("/tmp");
    });

    expect(selected).toBe("/home/me/Pictures");
    expect(invoke).toHaveBeenCalledWith("browse_folder", { defaultPath: "/tmp" });
  });
});

describe("useBrowseFile", () => {
  it("should call browse_file with filters and return the picked path", async () => {
    const invoke = await getInvoke();
    invoke.mockResolvedValue("/tmp/cookies.txt");

    const { result } = renderHook(() => useBrowseFile());
    let selected: string | null = null;
    await act(async () => {
      selected = await result.current({
        filters: [{ name: "Text", extensions: ["txt"] }],
      });
    });

    expect(selected).toBe("/tmp/cookies.txt");
    expect(invoke).toHaveBeenCalledWith("browse_file", {
      filters: [{ name: "Text", extensions: ["txt"] }],
      defaultPath: null,
    });
  });

  it("should return null when no arguments and user cancels", async () => {
    const invoke = await getInvoke();
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useBrowseFile());
    let selected: string | null = "x";
    await act(async () => {
      selected = await result.current();
    });

    expect(selected).toBeNull();
    expect(invoke).toHaveBeenCalledWith("browse_file", {
      filters: null,
      defaultPath: null,
    });
  });
});
