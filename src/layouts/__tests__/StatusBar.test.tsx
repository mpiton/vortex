import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StatusBar } from "../StatusBar";
import { useDownloadStore } from "@/stores/downloadStore";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

describe("StatusBar", () => {
  beforeEach(() => {
    useDownloadStore.setState({ countByState: {}, progressMap: {} });
  });

  it("should use the singular French label when one download is active", () => {
    window.localStorage.setItem("i18nextLng", "fr");
    useDownloadStore.setState({ countByState: { Downloading: 1 } });

    renderWithProviders(<StatusBar />);

    expect(screen.getByText("1 actif")).toBeInTheDocument();
  });

  it("should render download speed", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(/0\.0 MB\/s/)).toBeInTheDocument();
  });

  it("should render speed limit", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(/Limit: unlimited/)).toBeInTheDocument();
  });

  it("should render free space", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(/-- GB/)).toBeInTheDocument();
  });

  it("should render active download count", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(/0 active/)).toBeInTheDocument();
  });

  it("should render app version", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(/vortex v0\.1\.0/)).toBeInTheDocument();
  });

  it("should render clipboard indicator", () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText("Clipboard")).toBeInTheDocument();
  });
});
