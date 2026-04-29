import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useDownloadStore } from "@/stores/downloadStore";
import { useLayoutStore } from "@/stores/layout-store";
import { useUiStore } from "@/stores/uiStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { DownloadsView } from "@/views/DownloadsView";
import { LinkGrabberView } from "@/views/LinkGrabberView";
import { StatusBar } from "@/layouts/StatusBar";
import { PackagesView } from "@/views/PackagesView";
import { AccountsView } from "@/views/AccountsView";
import { CaptchaView } from "@/views/CaptchaView";
import { PluginsView } from "@/views/PluginsView";
import { SchedulerView } from "@/views/SchedulerView";
import { StatisticsView } from "@/views/StatisticsView";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
}

function renderWithProviders(ui: React.ReactElement, options: { tooltip?: boolean } = {}) {
  const content = options.tooltip ? <TooltipProvider>{ui}</TooltipProvider> : ui;

  return render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter>{content}</MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "fr");
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(async (command: string) => {
    switch (command) {
      case "download_list":
        return [];
      case "download_count_by_state":
        return {
          total: 0,
          Downloading: 0,
          Queued: 0,
          Completed: 0,
          Error: 0,
          Retry: 0,
        };
      case "clipboard_toggle":
        return true;
      default:
        return undefined;
    }
  });
  useUiStore.setState({
    selectedDownloadId: null,
    selectedDownloadIds: [],
    detailsPanelOpen: false,
    filterBarExpanded: false,
  });
  useSettingsStore.setState({ config: null, isLoading: false, error: null });
  useLayoutStore.setState({
    speedLimit: 0,
    freeSpace: "-- GB",
    appVersion: "0.1.0",
    sidebarCollapsed: false,
  });
  useDownloadStore.setState({ progressMap: {}, countByState: {} });
  cleanup();
});

describe("issue #30 — French UI translations", () => {
  it("renders downloads view strings in French", async () => {
    renderWithProviders(
      <div style={{ height: "600px" }}>
        <DownloadsView />
      </div>,
      { tooltip: true },
    );

    expect(await screen.findByText("Aucun téléchargement")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Rechercher des téléchargements...")).toBeInTheDocument();
    expect(screen.getByText("Tous")).toBeInTheDocument();
    expect(screen.getByText("Actifs")).toBeInTheDocument();
    expect(screen.getByText("En attente")).toBeInTheDocument();
    expect(screen.getByText("Terminés")).toBeInTheDocument();
    expect(screen.getByText("Échoués")).toBeInTheDocument();
    expect(screen.getByText("Tout suspendre")).toBeInTheDocument();
    expect(screen.getByText("Tout reprendre")).toBeInTheDocument();
  });

  it("renders link grabber shell strings in French", () => {
    renderWithProviders(<LinkGrabberView />, { tooltip: true });

    expect(screen.getByText("Capteur de liens")).toBeInTheDocument();
    expect(screen.getByText("Surveillance du presse-papiers")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Collez des URL ici (une par ligne)…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Effacer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Analyser les liens" })).toBeInTheDocument();
  });

  it("renders status bar strings in French", () => {
    renderWithProviders(<StatusBar />);

    expect(screen.getByText("Limite : illimitée")).toBeInTheDocument();
    expect(screen.getByText("-- GB libres")).toBeInTheDocument();
    expect(screen.getByText("Presse-papiers")).toBeInTheDocument();
    expect(screen.getByText("0 actif")).toBeInTheDocument();
  });

  it("renders placeholder views in French", () => {
    const views = [
      { component: <PackagesView />, title: "Paquets" },
      { component: <CaptchaView />, title: "Captcha" },
      { component: <SchedulerView />, title: "Planificateur" },
    ];

    for (const view of views) {
      const result = render(view.component);
      expect(screen.getByText(view.title)).toBeInTheDocument();
      expect(screen.getByText("Bientôt disponible")).toBeInTheDocument();
      result.unmount();
    }
  });

  it("renders the Accounts view header in French", () => {
    mockInvoke.mockResolvedValueOnce([]);
    renderWithProviders(<AccountsView />);
    expect(screen.getByRole("heading", { name: "Comptes" })).toBeInTheDocument();
  });

  it("renders plugin store catalogue shell in French", () => {
    mockInvoke.mockResolvedValue([]);
    renderWithProviders(<PluginsView />);

    expect(screen.getByPlaceholderText("Rechercher un plugin…")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Toutes" })).toBeInTheDocument();
  });

  it("renders statistics view strings in French", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      switch (command) {
        case "stats_get":
          return {
            totalDownloadedBytes: 0,
            totalFiles: 0,
            avgSpeed: 0,
            peakSpeed: 0,
            successRate: 1,
            dailyVolumes: [],
            topHosts: [],
          };
        case "stats_top_modules":
          return [];
        case "history_list":
          return [];
        default:
          return undefined;
      }
    });
    renderWithProviders(<StatisticsView />);

    expect(await screen.findByText("Statistiques")).toBeInTheDocument();
    expect(
      screen.getByText("Métriques locales de téléchargement sur la période sélectionnée"),
    ).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "7 derniers jours" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "30 derniers jours" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Tout" })).toBeInTheDocument();
    expect(screen.getByText("Volume total")).toBeInTheDocument();
    expect(screen.getByText("Fichiers")).toBeInTheDocument();
    expect(screen.getAllByText("Vitesse moyenne").length).toBeGreaterThan(0);
    expect(screen.getByText("Vitesse max")).toBeInTheDocument();
    expect(screen.getByText("Taux de succès")).toBeInTheDocument();
    expect(screen.getByText("Modules les plus utilisés (tout)")).toBeInTheDocument();
  });
});
