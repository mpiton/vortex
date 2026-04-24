import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SettingsView } from "../SettingsView";
import type { AppConfig } from "@/types/settings";
import { ThemeProvider } from "@/theme/theme-provider";

const mockInvoke = vi.hoisted(() => vi.fn());
const mockListen = vi.hoisted(() => vi.fn().mockResolvedValue(vi.fn()));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

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
  proxyType: "none",
  proxyUrl: null,
  userAgent: "Vortex/1.0",
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9876,
  restApiEnabled: true,
  apiKey: "test-api-key-123",
  websocketEnabled: true,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  theme: "auto",
  accentColor: "#4F46E5",
  compactMode: false,
  locale: "en",
};

function renderWithProviders() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <SettingsView />
      </ThemeProvider>
    </QueryClientProvider>,
  );
}

describe("SettingsView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(mockConfig);
    localStorage.clear();
    document.documentElement.classList.remove("dark");
  });

  it("should render all 7 tab buttons", async () => {
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: /Downloads/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Network/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Remote Access/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Browser/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Appearance/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Keyboard shortcuts/ })).toBeInTheDocument();

    const nav = screen.getByRole("navigation");
    expect(nav.querySelectorAll("button")).toHaveLength(7);

    expect(mockListen).toHaveBeenCalledWith("settings-updated", expect.any(Function));
  });

  it("should switch to Keyboard shortcuts section and list 10 shortcut rows", async () => {
    const user = userEvent.setup();
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Keyboard shortcuts/ }));

    const table = screen.getByRole("table");
    const rows = table.querySelectorAll("tbody tr");
    expect(rows.length).toBe(10);
    expect(screen.getByText(/Paste URLs into the link grabber/)).toBeInTheDocument();
    expect(screen.getByText(/Toggle clipboard monitoring/)).toBeInTheDocument();
  });

  it("should show General section by default", async () => {
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByText("Start minimized")).toBeInTheDocument();
    });

    expect(screen.getByText("Clipboard monitoring")).toBeInTheDocument();
  });

  it("should switch to Downloads section when tab clicked", async () => {
    const user = userEvent.setup();
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Downloads/ }));

    expect(screen.getByText("Max concurrent downloads")).toBeInTheDocument();
    expect(screen.getByText("Max segments per download")).toBeInTheDocument();
  });

  it("should switch to Network section when tab clicked", async () => {
    const user = userEvent.setup();
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Network/ }));

    expect(screen.getByText("Proxy type")).toBeInTheDocument();
    expect(screen.getByText("User agent")).toBeInTheDocument();
  });

  it("should switch to Appearance section when tab clicked", async () => {
    const user = userEvent.setup();
    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Appearance/ }));

    expect(screen.getByText("Theme")).toBeInTheDocument();
    expect(screen.getByText("Accent color")).toBeInTheDocument();
    expect(screen.getByText("Compact mode")).toBeInTheDocument();
  });

  it("should show loading skeletons when config not yet loaded", () => {
    mockInvoke.mockImplementation(() => new Promise(() => {}));

    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider>
          <SettingsView />
        </ThemeProvider>
      </QueryClientProvider>,
    );

    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
  });

  it("should show error state with message when settings_get fails", async () => {
    mockInvoke.mockRejectedValue(new Error("config store unavailable"));

    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByText("Failed to load settings")).toBeInTheDocument();
    });

    expect(screen.getByText("config store unavailable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("should recover when retry button is clicked after transient error", async () => {
    const user = userEvent.setup();
    mockInvoke.mockRejectedValueOnce(new Error("transient error"));

    renderWithProviders();

    await waitFor(() => {
      expect(screen.getByText("Failed to load settings")).toBeInTheDocument();
    });

    mockInvoke.mockResolvedValue(mockConfig);
    await user.click(screen.getByRole("button", { name: /Retry/ }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /General/ })).toBeInTheDocument();
    });
  });
});
