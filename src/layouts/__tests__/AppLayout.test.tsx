import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter, Routes, Route } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppLayout } from "../AppLayout";

const mockNavigate = vi.fn();
const originalPlatform = navigator.platform;

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
});
