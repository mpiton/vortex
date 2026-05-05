import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useDownloadStore } from "@/stores/downloadStore";
import { WaitCountdownCell } from "../WaitCountdownCell";

const invokeMock = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: Record<string, unknown>) => {
      if (options && typeof options.defaultValue === "string") {
        let out = options.defaultValue;
        if ("label" in options) {
          out = out.replace("{{label}}", String(options.label));
        }
        return out;
      }
      return _key;
    },
  }),
}));

function renderCell(downloadId: string) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <WaitCountdownCell downloadId={downloadId} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  invokeMock.mockClear();
  useDownloadStore.setState({ progressMap: {}, countByState: {}, waitMap: {} });
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-05-05T00:00:00Z"));
});

afterEach(() => {
  vi.useRealTimers();
});

describe("WaitCountdownCell", () => {
  it("should render a fallback when no wait ticket exists yet", () => {
    renderCell("42");
    expect(screen.getByText("Waiting…")).toBeInTheDocument();
  });

  it("should render the live countdown and the reason from the ticket", () => {
    useDownloadStore.getState().setWait("7", {
      untilUnixMs: Date.now() + 95_000,
      totalSeconds: 95,
      reason: "hoster cooldown",
    });
    renderCell("7");
    expect(screen.getByText("Wait 01:35")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Skip wait" })).toBeInTheDocument();
  });

  it("should call download_skip_wait when the skip button is clicked", async () => {
    vi.useRealTimers();
    useDownloadStore.getState().setWait("13", {
      untilUnixMs: Date.now() + 60_000,
      totalSeconds: 60,
      reason: "cooldown",
    });
    renderCell("13");
    fireEvent.click(screen.getByRole("button", { name: "Skip wait" }));
    // Mutation runs in a microtask — yield once so the mocked `invoke` lands.
    await Promise.resolve();
    expect(invokeMock).toHaveBeenCalledWith("download_skip_wait", { id: 13 });
  });
});
