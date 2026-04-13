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
import { HistoryView } from "@/views/HistoryView";
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

function renderWithProviders(
  ui: React.ReactElement,
  options: { tooltip?: boolean } = {},
) {
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
    expect(
      screen.getByPlaceholderText("Collez des URL ici (une par ligne)…"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Effacer" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Analyser les liens" }),
    ).toBeInTheDocument();
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
      { component: <AccountsView />, title: "Comptes" },
      { component: <CaptchaView />, title: "Captcha" },
      { component: <PluginsView />, title: "Plugins" },
      { component: <SchedulerView />, title: "Planificateur" },
      { component: <HistoryView />, title: "Historique" },
      { component: <StatisticsView />, title: "Statistiques" },
    ];

    for (const view of views) {
      const result = render(view.component);
      expect(screen.getByText(view.title)).toBeInTheDocument();
      expect(screen.getByText("Bientôt disponible")).toBeInTheDocument();
      result.unmount();
    }
  });
});
