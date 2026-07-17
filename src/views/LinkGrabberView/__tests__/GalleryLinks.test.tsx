import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { invoke } from "@tauri-apps/api/core";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LinkGrabberView } from "../LinkGrabberView";
import { useSettingsStore } from "@/stores/settingsStore";
import type { ResolvedLink } from "../types";

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

const GALLERY_URL = "https://imgur.com/a/abc123";

function galleryImageRow(id: string, url: string, filename: string | null): ResolvedLink {
  return {
    id,
    originalUrl: url,
    resolvedUrl: url,
    filename,
    sizeBytes: null,
    resumable: null,
    status: "online",
    errorMessage: null,
    errorKind: null,
    moduleName: "vortex-mod-gallery",
    accountId: null,
    isMedia: false,
    mediaType: "image",
    requiresOnlineProbe: false,
  };
}

function mockResolveWith(rows: ResolvedLink[]) {
  mockInvoke.mockImplementation((command, args) => {
    if (command === "link_resolve") return Promise.resolve(rows);
    if (command === "link_detect_duplicates") {
      const { urls } = args as { urls: string[] };
      return Promise.resolve(
        urls.map((url) => ({
          url,
          isDuplicate: false,
          source: null,
          existingId: null,
          existingFilename: null,
        })),
      );
    }
    return Promise.resolve(null);
  });
}

function renderWithProviders() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/link-grabber"]}>
        <TooltipProvider>
          <LinkGrabberView />
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

async function analyzeGallery(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByRole("textbox"), GALLERY_URL);
  await user.click(screen.getByRole("button", { name: "Analyze Links" }));
}

describe("LinkGrabberView gallery resolution", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue([]);
    vi.clearAllMocks();
    useSettingsStore.setState({ config: null });
  });

  it("should render gallery images in gallery order without a media button", async () => {
    mockResolveWith([
      galleryImageRow("img-1", "https://i.imgur.com/a.jpg", "imgur_abc123_000.jpg"),
      galleryImageRow("img-2", "https://i.imgur.com/b.png", "b.png"),
      galleryImageRow("img-3", "https://i.imgur.com/c.gif", "imgur_abc123_002.gif"),
    ]);

    const user = userEvent.setup();
    renderWithProviders();
    await analyzeGallery(user);

    await waitFor(() => {
      expect(screen.getByText("imgur_abc123_002.gif")).toBeInTheDocument();
    });
    const rowIds = screen
      .getAllByTestId(/^link-row-https/)
      .map((row) => row.getAttribute("data-testid"));
    expect(rowIds).toEqual([
      "link-row-https://i.imgur.com/a.jpg",
      "link-row-https://i.imgur.com/b.png",
      "link-row-https://i.imgur.com/c.gif",
    ]);
    expect(screen.getAllByText("vortex-mod-gallery")).toHaveLength(3);
    expect(screen.queryByRole("button", { name: "image" })).not.toBeInTheDocument();
  });

  it("should invoke download_start per selected image when Start Selected is clicked", async () => {
    mockResolveWith([
      galleryImageRow("img-1", "https://i.imgur.com/a.jpg", "imgur_abc123_000.jpg"),
      galleryImageRow("img-2", "https://i.imgur.com/b.png", "b.png"),
      galleryImageRow("img-3", "https://i.imgur.com/c.gif", "imgur_abc123_002.gif"),
    ]);

    const user = userEvent.setup();
    renderWithProviders();
    await analyzeGallery(user);

    await waitFor(() => {
      expect(screen.getByText("imgur_abc123_002.gif")).toBeInTheDocument();
    });
    for (const url of ["https://i.imgur.com/a.jpg", "https://i.imgur.com/c.gif"]) {
      const row = screen.getByTestId(`link-row-${url}`);
      await user.click(within(row).getByRole("checkbox", { name: "Select link" }));
    }
    await user.click(screen.getByRole("button", { name: "Start Selected (2)" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "download_start",
        expect.objectContaining({
          url: "https://i.imgur.com/a.jpg",
          moduleName: "vortex-mod-gallery",
        }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "download_start",
        expect.objectContaining({ url: "https://i.imgur.com/c.gif" }),
      );
    });
    const startedUrls = mockInvoke.mock.calls
      .filter(([command]) => command === "download_start")
      .map(([, args]) => (args as { url: string }).url);
    expect(startedUrls).not.toContain("https://i.imgur.com/b.png");
  });

  it("should show a per-item error row without blocking the valid gallery images", async () => {
    const errorRow: ResolvedLink = {
      id: "img-err",
      originalUrl: GALLERY_URL,
      resolvedUrl: null,
      filename: null,
      sizeBytes: null,
      resumable: null,
      status: "error",
      errorMessage: "Gallery item 'broken slide' has an invalid image URL",
      errorKind: "plugin",
      moduleName: "vortex-mod-gallery",
      accountId: null,
      isMedia: false,
      requiresOnlineProbe: false,
    };
    mockResolveWith([
      galleryImageRow("img-1", "https://i.imgur.com/a.jpg", "imgur_abc123_000.jpg"),
      errorRow,
      galleryImageRow("img-3", "https://i.imgur.com/c.jpg", "imgur_abc123_002.jpg"),
    ]);

    const user = userEvent.setup();
    renderWithProviders();
    await analyzeGallery(user);

    await waitFor(() => {
      expect(
        screen.getByText("Gallery item 'broken slide' has an invalid image URL"),
      ).toBeInTheDocument();
    });
    await user.click(screen.getByRole("button", { name: "Start All Online" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "download_start",
        expect.objectContaining({ url: "https://i.imgur.com/a.jpg" }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "download_start",
        expect.objectContaining({ url: "https://i.imgur.com/c.jpg" }),
      );
    });
    const startCalls = mockInvoke.mock.calls.filter(([command]) => command === "download_start");
    expect(startCalls).toHaveLength(2);
  });
});
