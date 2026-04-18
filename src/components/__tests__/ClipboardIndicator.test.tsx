import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { invoke, listen, clearMockListeners } from "@/test/__mocks__/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { ClipboardIndicator } from "../ClipboardIndicator";
import { useSettingsStore } from "@/stores/settingsStore";
import type { AppConfig } from "@/types/settings";

const baseConfig: AppConfig = {
  downloadDir: null,
  maxConcurrentDownloads: 4,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  autoExtract: true,
  theme: "dark",
  locale: "en",
  clipboardMonitoring: false,
  startMinimized: false,
  notificationsEnabled: true,
  soundEnabled: false,
  confirmDelete: true,
  subfolderPerPackage: false,
  maxRetries: 5,
  retryDelaySeconds: 10,
  verifyChecksums: true,
  preAllocateSpace: false,
  proxyType: "none",
  proxyUrl: null,
  userAgent: "Vortex/1.0",
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9876,
  restApiEnabled: true,
  apiKey: "",
  websocketEnabled: true,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  accentColor: "#4F46E5",
  compactMode: false,
};

function wrapper({ children }: { children: React.ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("ClipboardIndicator", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearMockListeners();
    useSettingsStore.setState({ config: null });
  });

  it("should render with clipboard label", () => {
    render(<ClipboardIndicator />, { wrapper });
    expect(screen.getByText("Clipboard")).toBeInTheDocument();
  });

  it("should show grey dot when disabled", () => {
    useSettingsStore.setState({
      config: { ...baseConfig, clipboardMonitoring: false },
    });

    render(<ClipboardIndicator />, { wrapper });
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute(
      "title",
      "Clipboard monitoring paused"
    );
  });

  it("should show green dot when enabled", () => {
    useSettingsStore.setState({
      config: { ...baseConfig, clipboardMonitoring: true },
    });

    render(<ClipboardIndicator />, { wrapper });
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute(
      "title",
      "Clipboard monitoring active"
    );
  });

  it("should call clipboard_toggle IPC on click", async () => {
    const user = userEvent.setup();
    invoke.mockResolvedValue(true);

    render(<ClipboardIndicator />, { wrapper });
    await user.click(screen.getByRole("button"));

    expect(invoke).toHaveBeenCalledWith("clipboard_toggle", {
      enabled: true,
    });
  });
});
