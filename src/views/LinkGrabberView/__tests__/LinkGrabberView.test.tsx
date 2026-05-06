import { beforeEach, describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "@/lib/toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LinkGrabberView } from "../LinkGrabberView";
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
  dynamicSplitEnabled: true,
  dynamicSplitMinRemainingMb: 4,
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
  historyRetentionDays: 30,
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const mockInvoke = vi.mocked(invoke);

function renderWithProviders(
  initialEntry: string | { pathname: string; state?: unknown } = "/link-grabber",
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <TooltipProvider>
          <LinkGrabberView />
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("LinkGrabberView", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue([]);
    vi.clearAllMocks();
    useSettingsStore.setState({ config: null });
  });

  it("should render the header title", () => {
    renderWithProviders();
    expect(screen.getByText("Link Grabber")).toBeInTheDocument();
  });

  it("should render the clipboard monitoring toggle", () => {
    renderWithProviders();
    expect(screen.getByText("Clipboard Monitoring")).toBeInTheDocument();
    expect(screen.getByRole("switch")).toBeInTheDocument();
  });

  it("should not disable the clipboard switch", () => {
    renderWithProviders();
    expect(screen.getByRole("switch")).not.toBeDisabled();
  });

  it("should invoke clipboard_toggle when switch is clicked", async () => {
    mockInvoke.mockResolvedValue(true);
    const user = userEvent.setup();
    renderWithProviders();

    await user.click(screen.getByRole("switch"));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("clipboard_toggle", {
        enabled: true,
      });
    });
  });

  it("should reflect initial enabled state from settings store", () => {
    useSettingsStore.setState({
      config: { ...baseConfig, clipboardMonitoring: true },
    });
    renderWithProviders();
    expect(screen.getByRole("switch")).toBeChecked();
  });

  it("should show active tooltip and green dot when monitoring is enabled", () => {
    useSettingsStore.setState({
      config: { ...baseConfig, clipboardMonitoring: true },
    });
    renderWithProviders();
    expect(screen.getByTitle("Clipboard monitoring active")).toBeInTheDocument();
    expect(
      document.querySelector('[data-testid="clipboard-status-dot"].bg-success'),
    ).not.toBeNull();
  });

  it("should show paused tooltip when monitoring is disabled", () => {
    useSettingsStore.setState({
      config: { ...baseConfig, clipboardMonitoring: false },
    });
    renderWithProviders();
    expect(screen.getByTitle("Clipboard monitoring paused")).toBeInTheDocument();
    expect(document.querySelector('[data-testid="clipboard-status-dot"].bg-border')).not.toBeNull();
  });

  it("should render PasteZone with Analyze Links button", () => {
    renderWithProviders();
    expect(screen.getByRole("button", { name: "Analyze Links" })).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("should not show filter/grouping/actions sections when no links resolved", () => {
    renderWithProviders();
    expect(screen.queryByRole("button", { name: "All" })).not.toBeInTheDocument();
    expect(screen.queryByText("Group Into Packages:")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Select All/ })).not.toBeInTheDocument();
  });

  it("should call link_resolve when Analyze Links is clicked", async () => {
    mockInvoke.mockResolvedValue([]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "link_resolve",
        expect.objectContaining({ urls: ["https://example.com/file.zip"] }),
      );
    });
  });

  it("should surface error toast on failure and success toast on retry", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("AppState not registered")).mockResolvedValueOnce([
      {
        id: "1",
        originalUrl: "https://example.com/file.zip",
        resolvedUrl: "https://example.com/file.zip",
        filename: "file.zip",
        sizeBytes: 1024,
        status: "online",
        moduleName: "http",
        isMedia: false,
      },
    ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("AppState not registered");
    });

    // Clear the textarea and retry
    await user.clear(textarea);
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(toast.success).toHaveBeenCalled();
      expect(screen.getByRole("button", { name: "Select All (1)" })).toBeInTheDocument();
    });
  });

  it("should show filter/grouping/actions sections after links are resolved", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "1",
        originalUrl: "https://example.com/file.zip",
        resolvedUrl: "https://example.com/file.zip",
        filename: "file.zip",
        sizeBytes: 1024,
        status: "online",
        moduleName: "http",
        isMedia: false,
      },
    ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "All" })).toBeInTheDocument();
    });

    expect(screen.getByText("Group Into Packages:")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select All (1)" })).toBeInTheDocument();
  });

  it("should clear resolved links when Clear is clicked", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "1",
        originalUrl: "https://example.com/file.zip",
        resolvedUrl: "https://example.com/file.zip",
        filename: "file.zip",
        sizeBytes: 1024,
        status: "online",
        moduleName: "http",
        isMedia: false,
      },
    ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Select All (1)" })).toBeInTheDocument();
    });

    const clearButtons = screen.getAllByRole("button", { name: "Clear" });
    // ActionsBar "Clear" button is the destructive one (last among Clear buttons)
    await user.click(clearButtons[clearButtons.length - 1]);

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "All" })).not.toBeInTheDocument();
    });
  });

  it("should focus the paste textarea when opened from the global add-urls shortcut", async () => {
    renderWithProviders({
      pathname: "/link-grabber",
      state: { focusPaste: true },
    });

    await waitFor(() => {
      expect(screen.getByRole("textbox")).toHaveFocus();
    });
  });

  it("should call link_import_container and resolve returned URLs when a .dlc is dropped", async () => {
    mockInvoke.mockImplementation((cmd) => {
      if (cmd === "link_import_container") {
        return Promise.resolve({
          format: "dlc",
          fileName: "pack.dlc",
          urls: ["https://hoster.example/a.bin", "https://hoster.example/b.bin"],
          packageId: "pkg-1",
          packageName: "pack.dlc",
        });
      }
      return Promise.resolve([]);
    });

    renderWithProviders();

    const dropZone = screen.getByTestId("paste-drop-zone");
    const dlcBytes = new Uint8Array([0x44, 0x4c, 0x43, 0x00]); // "DLC\0"
    const dlcFile = new File([dlcBytes], "pack.dlc", {
      type: "application/octet-stream",
    });

    fireEvent.drop(dropZone, {
      dataTransfer: {
        files: [dlcFile],
        getData: () => "",
      },
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "link_import_container",
        expect.objectContaining({
          fileName: "pack.dlc",
          fileBytes: expect.any(Array),
        }),
      );
    });

    const importCall = mockInvoke.mock.calls.find(([c]) => c === "link_import_container");
    const importedBytes = (importCall![1] as { fileBytes: number[] }).fileBytes;
    expect(importedBytes).toEqual([0x44, 0x4c, 0x43, 0x00]);

    await waitFor(() => {
      expect(toast.success).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "link_resolve",
        expect.objectContaining({
          urls: ["https://hoster.example/a.bin", "https://hoster.example/b.bin"],
        }),
      );
    });
  });

  it("should toast an error when link_import_container fails", async () => {
    mockInvoke.mockImplementation((cmd) => {
      if (cmd === "link_import_container") {
        return Promise.reject("Install vortex-mod-containers");
      }
      return Promise.resolve([]);
    });

    renderWithProviders();

    const dropZone = screen.getByTestId("paste-drop-zone");
    const dlcFile = new File([new Uint8Array([1, 2, 3])], "pack.dlc", {
      type: "application/octet-stream",
    });

    fireEvent.drop(dropZone, {
      dataTransfer: {
        files: [dlcFile],
        getData: () => "",
      },
    });

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalled();
    });
    const linkResolveCalls = mockInvoke.mock.calls.filter(([c]) => c === "link_resolve");
    expect(linkResolveCalls).toHaveLength(0);
  });

  it("should pre-fill the textarea and resolve links when opened with pasteContent", async () => {
    mockInvoke.mockResolvedValue([]);
    renderWithProviders({
      pathname: "/link-grabber",
      state: {
        focusPaste: true,
        pasteContent: "https://example.com/a.zip\nhttps://example.com/b.zip",
        pasteToken: "token-abc-123",
      },
    });

    await waitFor(() => {
      const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
      expect(textarea.value).toContain("https://example.com/a.zip");
      expect(textarea.value).toContain("https://example.com/b.zip");
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "link_resolve",
        expect.objectContaining({
          urls: ["https://example.com/a.zip", "https://example.com/b.zip"],
        }),
      );
    });
  });
});
