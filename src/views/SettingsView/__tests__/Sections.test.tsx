import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { GeneralSection } from "../GeneralSection";
import { DownloadsSection } from "../DownloadsSection";
import { NetworkSection } from "../NetworkSection";
import { RemoteAccessSection } from "../RemoteAccessSection";
import { BrowserSection } from "../BrowserSection";
import { AppearanceSection } from "../AppearanceSection";
import type { AppConfig } from "@/types/settings";
import { ThemeProvider } from "@/theme/theme-provider";

const mockInvoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

beforeAll(() => {
  Element.prototype.hasPointerCapture = vi.fn().mockReturnValue(false);
  Element.prototype.setPointerCapture = vi.fn();
  Element.prototype.releasePointerCapture = vi.fn();
  Element.prototype.scrollIntoView = vi.fn();
});

const mockConfig: AppConfig = {
  downloadDir: "/tmp/downloads",
  startMinimized: false,
  notificationsEnabled: true,
  autoExtract: true,
  clipboardMonitoring: true,
  soundEnabled: false,
  confirmDelete: true,
  subfolderPerPackage: false,
  maxConcurrentDownloads: 4,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  maxRetries: 5,
  retryDelaySeconds: 10,
  verifyChecksums: true,
  preAllocateSpace: true,
  dynamicSplitEnabled: true,
  dynamicSplitMinRemainingMb: 4,
  captchaTimeoutSeconds: 120,
  captchaSolverOrder: [
    "vortex-mod-captcha-ocr",
    "vortex-mod-captcha-anticaptcha",
    "vortex-mod-captcha-browser",
  ],
  historyRetentionDays: 30,
  proxyType: "none",
  proxyUrl: null,
  userAgent: "Vortex/1.0",
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9876,
  restApiEnabled: true,
  apiKey: "test-api-key-abc-123",
  websocketEnabled: true,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  theme: "auto",
  accentColor: "#4F46E5",
  compactMode: false,
  locale: "en",
};

function renderWithQuery(children: React.ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>);
}

function renderWithTheme(children: React.ReactNode) {
  return renderWithQuery(<ThemeProvider>{children}</ThemeProvider>);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockInvoke.mockResolvedValue(null);
  localStorage.clear();
  document.documentElement.classList.remove("dark");
});

describe("GeneralSection", () => {
  it("should render download directory input", () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByDisplayValue("/tmp/downloads")).toBeInTheDocument();
  });

  it("should render all toggle settings", () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByText("Start minimized")).toBeInTheDocument();
    expect(screen.getByText("Notifications")).toBeInTheDocument();
    expect(screen.getByText("Auto extract")).toBeInTheDocument();
    expect(screen.getByText("Clipboard monitoring")).toBeInTheDocument();
    expect(screen.getByText("Sound effects")).toBeInTheDocument();
    expect(screen.getByText("Confirm before delete")).toBeInTheDocument();
    expect(screen.getByText("Subfolder per package")).toBeInTheDocument();
  });

  it("should render the history retention dropdown bound to the current value", () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    const trigger = screen.getByRole("combobox", { name: "History retention" });
    expect(trigger).toHaveTextContent("30 days");
  });

  it("should persist a new history retention value via settings_update", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "settings_update") return null;
      return null;
    });
    const user = userEvent.setup();
    renderWithQuery(<GeneralSection config={mockConfig} />);
    const trigger = screen.getByRole("combobox", { name: "History retention" });
    await user.click(trigger);
    await user.click(await screen.findByRole("option", { name: "Never" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "settings_update",
        expect.objectContaining({ patch: { historyRetentionDays: 0 } }),
      );
    });
  });

  it("should render Browse button for directory picker", () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByLabelText("Browse")).toBeInTheDocument();
  });

  it("should render Browse button enabled", () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    const btn = screen.getByLabelText("Browse");
    expect(btn).not.toBeDisabled();
  });

  it("should call browse_folder and persist selection on click", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "browse_folder") return "/new/downloads";
      if (command === "settings_update") return null;
      return null;
    });
    const user = userEvent.setup();
    renderWithQuery(<GeneralSection config={mockConfig} />);
    await user.click(screen.getByLabelText("Browse"));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "browse_folder",
        expect.objectContaining({ defaultPath: "/tmp/downloads" }),
      );
    });
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "settings_update",
        expect.objectContaining({ patch: { downloadDir: "/new/downloads" } }),
      );
    });
  });

  it("should not update settings when user cancels the dialog", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "browse_folder") return null;
      return null;
    });
    const user = userEvent.setup();
    renderWithQuery(<GeneralSection config={mockConfig} />);
    await user.click(screen.getByLabelText("Browse"));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("browse_folder", expect.anything());
    });
    const updateCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "settings_update");
    expect(updateCalls).toHaveLength(0);
  });

  it("should skip settings_update when picked folder equals the current one", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "browse_folder") return "/tmp/downloads";
      return null;
    });
    const user = userEvent.setup();
    renderWithQuery(<GeneralSection config={mockConfig} />);
    await user.click(screen.getByLabelText("Browse"));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("browse_folder", expect.anything());
    });
    const updateCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "settings_update");
    expect(updateCalls).toHaveLength(0);
  });

  it("should prevent concurrent browse_folder calls while dialog is pending", async () => {
    let resolveFolder: ((value: string | null) => void) | undefined;
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "browse_folder") {
        return new Promise<string | null>((resolve) => {
          resolveFolder = resolve;
        });
      }
      return null;
    });
    const user = userEvent.setup();
    renderWithQuery(<GeneralSection config={mockConfig} />);
    const btn = screen.getByLabelText("Browse");

    await user.click(btn);
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("browse_folder", expect.anything());
    });
    await waitFor(() => expect(btn).toBeDisabled());

    await user.click(btn);
    const browseCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "browse_folder");
    expect(browseCalls).toHaveLength(1);

    resolveFolder?.(null);
    await waitFor(() => expect(btn).not.toBeDisabled());
  });
});

describe("DownloadsSection", () => {
  it("should render number inputs with correct values", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByText("Max concurrent downloads")).toBeInTheDocument();
    expect(screen.getByText("Max segments per download")).toBeInTheDocument();
    expect(screen.getByText("Speed limit (MiB/s)")).toBeInTheDocument();
  });

  it("should render toggle settings", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByText("Verify checksums")).toBeInTheDocument();
    expect(screen.getByText("Pre-allocate space")).toBeInTheDocument();
  });

  it("should cap maxConcurrentDownloads input at 20 per PRD §6.10", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    const input = screen.getByLabelText<HTMLInputElement>("Max concurrent downloads");
    expect(input.max).toBe("20");
  });

  // MAT-136 R-02: throttling and pre-allocation are not consumed by the
  // engine yet — the controls must be non-interactive and marked planned.
  it("should disable speed limit input when throttling is not implemented", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByLabelText("Speed limit (MiB/s)")).toBeDisabled();
  });

  it("should disable pre-allocate toggle when setting is not consumed", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByRole("switch", { name: "Pre-allocate space" })).toBeDisabled();
  });

  it("should mark planned download settings with coming soon badge", () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getAllByText("Coming soon")).toHaveLength(2);
  });
});

describe("NetworkSection", () => {
  it("should render proxy type selector", () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByText("Proxy type")).toBeInTheDocument();
  });

  it("should not show proxy URL when proxy type is none", () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.queryByText("Proxy URL")).not.toBeInTheDocument();
  });

  it("should show proxy URL when proxy type is http", () => {
    renderWithQuery(<NetworkSection config={{ ...mockConfig, proxyType: "http" }} />);
    expect(screen.getByPlaceholderText("http://proxy:8080")).toBeInTheDocument();
  });

  it("should render DNS over HTTPS toggle", () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByText("DNS over HTTPS")).toBeInTheDocument();
  });

  // MAT-136 R-02: DoH is not consumed by the HTTP client yet.
  it("should disable DNS over HTTPS toggle when setting is not consumed", () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByRole("switch", { name: "DNS over HTTPS" })).toBeDisabled();
  });

  it("should show restart hint when network settings apply at launch", () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByText(/next launch/i)).toBeInTheDocument();
  });
});

describe("RemoteAccessSection", () => {
  // MAT-136 R-03: no server exists yet, the section must never look active.
  it("should render planned notice when section is shown", () => {
    renderWithQuery(<RemoteAccessSection />);
    expect(screen.getByText(/planned for a future release/i)).toBeInTheDocument();
  });

  it("should not render interactive controls when no server exists", () => {
    renderWithQuery(<RemoteAccessSection />);
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByText("API Key")).not.toBeInTheDocument();
  });
});

describe("BrowserSection", () => {
  it("should render min file size input", () => {
    renderWithQuery(<BrowserSection config={mockConfig} />);
    expect(screen.getByText("Minimum file size (MB)")).toBeInTheDocument();
  });

  it("should render domain and extension textareas", () => {
    renderWithQuery(<BrowserSection config={mockConfig} />);
    expect(screen.getByText("Excluded domains")).toBeInTheDocument();
    expect(screen.getByText("Excluded extensions")).toBeInTheDocument();
  });
});

describe("AppearanceSection", () => {
  it("should render theme selector", () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText("Theme")).toBeInTheDocument();
  });

  it("should apply dark theme immediately when dark is selected", async () => {
    const user = userEvent.setup();

    renderWithTheme(<AppearanceSection config={mockConfig} />);

    await user.click(screen.getByRole("combobox", { name: "Theme" }));
    await user.click(await screen.findByRole("option", { name: "Dark" }));

    await waitFor(() => {
      expect(localStorage.getItem("vortex-theme")).toBe("dark");
      expect(document.documentElement).toHaveClass("dark");
    });
  });

  it("should keep the current theme when the theme mutation fails", async () => {
    const user = userEvent.setup();
    mockInvoke.mockRejectedValueOnce(new Error("config store unavailable"));

    renderWithTheme(<AppearanceSection config={mockConfig} />);

    await user.click(screen.getByRole("combobox", { name: "Theme" }));
    await user.click(await screen.findByRole("option", { name: "Dark" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("settings_update", { patch: { theme: "dark" } });
    });

    expect(localStorage.getItem("vortex-theme")).toBe("auto");
    expect(document.documentElement).not.toHaveClass("dark");
  });

  it("should render 6 accent color buttons", () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByLabelText("Indigo")).toBeInTheDocument();
    expect(screen.getByLabelText("Blue")).toBeInTheDocument();
    expect(screen.getByLabelText("Purple")).toBeInTheDocument();
    expect(screen.getByLabelText("Pink")).toBeInTheDocument();
    expect(screen.getByLabelText("Red")).toBeInTheDocument();
    expect(screen.getByLabelText("Green")).toBeInTheDocument();
  });

  it("should render compact mode toggle", () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText("Compact mode")).toBeInTheDocument();
  });

  it("should render language selector", () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText("Language")).toBeInTheDocument();
  });
});
