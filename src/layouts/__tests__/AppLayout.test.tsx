import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { MemoryRouter, Routes, Route } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { AppLayout } from "../AppLayout";
import { useUiStore } from "@/stores/uiStore";
import { useSettingsStore } from "@/stores/settingsStore";
import type { AppConfig } from "@/types/settings";

const mockNavigate = vi.fn();
const originalPlatform = navigator.platform;
const mockInvoke = vi.mocked(invoke);

const baseConfig: AppConfig = {
  downloadDir: null,
  maxConcurrentDownloads: 3,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  autoExtract: false,
  theme: "dark",
  locale: "en",
  clipboardMonitoring: false,
  startMinimized: false,
  notificationsEnabled: true,
  soundEnabled: false,
  confirmDelete: true,
  subfolderPerPackage: false,
  maxRetries: 3,
  retryDelaySeconds: 5,
  verifyChecksums: true,
  preAllocateSpace: false,
  proxyType: "none",
  proxyUrl: null,
  userAgent: "Vortex/1.0",
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9666,
  restApiEnabled: false,
  apiKey: "",
  websocketEnabled: false,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  accentColor: "#4F46E5",
  compactMode: false,
};

vi.mock("@/hooks/useDownloadProgress", () => ({
  useDownloadProgress: vi.fn(),
}));

vi.mock("@/hooks/useDownloadEvents", () => ({
  useDownloadEvents: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("react-router", async () => {
  const actual = await vi.importActual("react-router");
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

function renderAppLayout(initialRoute = "/downloads") {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialRoute]}>
        <Routes>
          <Route element={<AppLayout />}>
            <Route path="downloads" element={<div>Downloads Page</div>} />
            <Route path="link-grabber" element={<div>Link Grabber Page</div>} />
            <Route path="settings" element={<div>Settings Page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("AppLayout", () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    mockInvoke.mockReset();
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "settings_get") {
        return { ...baseConfig };
      }
      return undefined;
    });
    useUiStore.setState({
      selectedDownloadId: null,
      selectedDownloadIds: [],
      detailsPanelOpen: false,
      filterBarExpanded: false,
    });
    useSettingsStore.setState({
      config: { ...baseConfig },
      isLoading: false,
      error: null,
    });
  });

  afterEach(() => {
    Object.defineProperty(navigator, "platform", {
      value: originalPlatform,
      configurable: true,
    });
  });

  it("should render Sidebar, main content, and StatusBar", () => {
    renderAppLayout();
    expect(screen.getByText("Vx")).toBeInTheDocument();
    expect(screen.getByText("Downloads Page")).toBeInTheDocument();
    expect(screen.getByText(/vortex v0\.1\.0/)).toBeInTheDocument();
  });

  it("should apply the backend locale on startup", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "settings_get") {
        return { ...baseConfig, locale: "fr" };
      }
      return undefined;
    });

    renderAppLayout();

    await waitFor(() => {
      expect(screen.getByText("Limite : illimitée")).toBeInTheDocument();
    });
  });

  it.each([
    { platform: "Linux x86_64", modifier: "ctrlKey" as const },
    { platform: "MacIntel", modifier: "metaKey" as const },
  ])("should navigate on $modifier+1 ($platform)", ({ platform, modifier }) => {
    Object.defineProperty(navigator, "platform", { value: platform, configurable: true });
    renderAppLayout();
    fireEvent.keyDown(window, { key: "1", [modifier]: true });
    expect(mockNavigate).toHaveBeenCalledWith("/downloads");
  });

  it.each([
    { platform: "Linux x86_64", modifier: "ctrlKey" as const },
    { platform: "MacIntel", modifier: "metaKey" as const },
  ])("should navigate to settings on $modifier+, ($platform)", ({ platform, modifier }) => {
    Object.defineProperty(navigator, "platform", { value: platform, configurable: true });
    renderAppLayout();
    fireEvent.keyDown(window, { key: ",", [modifier]: true });
    expect(mockNavigate).toHaveBeenCalledWith("/settings");
  });

  it("should ignore keydown without modifier", () => {
    renderAppLayout();
    fireEvent.keyDown(window, { key: "1" });
    expect(mockNavigate).not.toHaveBeenCalled();
  });

  it("should dispatch downloads focus shortcut on Ctrl+F", () => {
    const shortcutSpy = vi.fn();
    const listener = (event: Event) => {
      shortcutSpy((event as CustomEvent<string>).detail);
    };

    window.addEventListener("vortex:shortcut-action", listener as EventListener);

    try {
      renderAppLayout();
      fireEvent.keyDown(window, { key: "f", ctrlKey: true });
      expect(shortcutSpy).toHaveBeenCalledWith("downloads.focus-search");
    } finally {
      window.removeEventListener("vortex:shortcut-action", listener as EventListener);
    }
  });

  it("should navigate to link grabber on Ctrl+N", () => {
    renderAppLayout();

    fireEvent.keyDown(window, { key: "n", ctrlKey: true });

    expect(mockNavigate).toHaveBeenCalledWith("/link-grabber", {
      replace: false,
      state: { focusPaste: true },
    });
  });

  it("should toggle clipboard monitoring on Ctrl+Shift+P", async () => {
    mockInvoke.mockResolvedValue(true);
    renderAppLayout();

    fireEvent.keyDown(window, { key: "P", ctrlKey: true, shiftKey: true });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("clipboard_toggle", {
        enabled: true,
      });
    });
  });

  it("should close the details panel on Escape", () => {
    useUiStore.setState({
      selectedDownloadId: "download-1",
      selectedDownloadIds: ["download-1"],
      detailsPanelOpen: true,
      filterBarExpanded: false,
    });

    renderAppLayout();
    fireEvent.keyDown(window, { key: "Escape" });

    expect(useUiStore.getState().detailsPanelOpen).toBe(false);
  });
});
