import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useUiStore } from "@/stores/uiStore";
import { useDownloadStore } from "@/stores/downloadStore";
import type { DownloadView } from "@/types/download";
import { DownloadsTable, computeReorderedIds } from "../DownloadsTable";

const invokeMock = vi.fn().mockResolvedValue(undefined);
const toastErrorMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        index: i,
        start: i * 48,
        end: (i + 1) * 48,
        size: 48,
        key: i,
      })),
    getTotalSize: () => count * 48,
    measureElement: () => undefined,
  }),
}));

const MOCK_DOWNLOADS: DownloadView[] = [
  {
    id: "1",
    fileName: "file1.zip",
    url: "https://example.com/file1.zip",
    sourceHostname: "example.com",
    state: "Downloading",
    progressPercent: 50,
    speedBytesPerSec: 1024 * 1024,
    downloadedBytes: 5000,
    totalBytes: 10000,
    etaSeconds: 5,
    segmentsActive: 2,
    segmentsTotal: 4,
    moduleName: null,
    accountName: null,
    priority: 5,
    queuePosition: 0,
    createdAt: Date.now(),
  },
  {
    id: "2",
    fileName: "image.png",
    url: "https://cdn.test.org/image.png",
    sourceHostname: "cdn.test.org",
    state: "Completed",
    progressPercent: 100,
    speedBytesPerSec: 0,
    downloadedBytes: 20000,
    totalBytes: 20000,
    etaSeconds: null,
    segmentsActive: 0,
    segmentsTotal: 1,
    moduleName: null,
    accountName: null,
    priority: 5,
    queuePosition: 0,
    createdAt: Date.now(),
  },
  {
    id: "3",
    fileName: "video.mp4",
    url: "https://media.example.com/video.mp4",
    sourceHostname: "media.example.com",
    state: "Error",
    errorMessage: "tls handshake failed",
    progressPercent: 30,
    speedBytesPerSec: 0,
    downloadedBytes: 3000,
    totalBytes: 10000,
    etaSeconds: null,
    segmentsActive: 0,
    segmentsTotal: 4,
    moduleName: null,
    accountName: null,
    priority: 5,
    queuePosition: 0,
    createdAt: Date.now(),
  },
];

function renderTable(
  props: Partial<{
    downloads: DownloadView[];
    isLoading: boolean;
    filter: "all" | "active" | "queued" | "done" | "failed";
    searchQuery: string;
  }> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <div style={{ height: "600px", overflow: "auto" }}>
          <DownloadsTable
            downloads={props.downloads ?? MOCK_DOWNLOADS}
            isLoading={props.isLoading ?? false}
            filter={props.filter ?? "all"}
            searchQuery={props.searchQuery ?? ""}
          />
        </div>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  // Some specs switch to `fr` to assert localized labels; reset to `en`
  // up-front so later specs asserting English strings don't depend on the
  // test execution order.
  window.localStorage.setItem("i18nextLng", "en");
  useUiStore.setState({
    selectedDownloadId: null,
    selectedDownloadIds: [],
    detailsPanelOpen: false,
    filterBarExpanded: false,
  });
  useDownloadStore.setState({ progressMap: {} });
  invokeMock.mockClear();
  invokeMock.mockResolvedValue(undefined);
  toastErrorMock.mockClear();
});

describe("DownloadsTable", () => {
  it("should render download filenames", () => {
    renderTable();
    expect(screen.getByText("file1.zip")).toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
    expect(screen.getByText("video.mp4")).toBeInTheDocument();
  });

  it("should show loading state", () => {
    renderTable({ isLoading: true });
    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  it("should show empty state when no downloads", () => {
    renderTable({ downloads: [] });
    expect(screen.getByText("No downloads")).toBeInTheDocument();
  });

  it("should filter by active state", () => {
    renderTable({ filter: "active" });
    expect(screen.getByText("file1.zip")).toBeInTheDocument();
    expect(screen.queryByText("image.png")).not.toBeInTheDocument();
    expect(screen.queryByText("video.mp4")).not.toBeInTheDocument();
  });

  it("should filter by done state", () => {
    renderTable({ filter: "done" });
    expect(screen.queryByText("file1.zip")).not.toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
  });

  it("should filter by search query", () => {
    renderTable({ searchQuery: "image" });
    expect(screen.queryByText("file1.zip")).not.toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
  });

  it("should search by hostname", () => {
    renderTable({ searchQuery: "cdn.test" });
    expect(screen.queryByText("file1.zip")).not.toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
  });

  it("should render column headers", () => {
    renderTable();
    expect(screen.getByText("Filename")).toBeInTheDocument();
    expect(screen.getByText("State")).toBeInTheDocument();
    expect(screen.getByText("Progress")).toBeInTheDocument();
    expect(screen.getByText("Speed")).toBeInTheDocument();
    expect(screen.getByText("ETA")).toBeInTheDocument();
  });

  it("should render French column headers when locale is fr", () => {
    window.localStorage.setItem("i18nextLng", "fr");

    renderTable();

    expect(screen.getByText("Nom du fichier")).toBeInTheDocument();
    expect(screen.getByText("État")).toBeInTheDocument();
    expect(screen.getByText("Type")).toBeInTheDocument();
    expect(screen.getByText("Hôte")).toBeInTheDocument();
    expect(screen.getByText("Progression")).toBeInTheDocument();
    expect(screen.getByText("Vitesse")).toBeInTheDocument();
    expect(screen.getByText("Temps restant")).toBeInTheDocument();
  });

  it("should render French action menu labels when locale is fr", async () => {
    window.localStorage.setItem("i18nextLng", "fr");
    const user = userEvent.setup();

    renderTable();

    await user.click(screen.getAllByLabelText("Plus d’actions")[0]);
    await user.click(screen.getByText("Définir la priorité"));

    expect(screen.getByText("Définir la priorité")).toBeInTheDocument();
    expect(screen.getByText("Haute")).toBeInTheDocument();
    expect(screen.getByText("Normale")).toBeInTheDocument();
    expect(screen.getByText("Basse")).toBeInTheDocument();
    expect(screen.getByText("Supprimer")).toBeInTheDocument();
  });

  it("should extract and show file type badge", () => {
    renderTable();
    expect(screen.getByText("ZIP")).toBeInTheDocument();
    expect(screen.getByText("PNG")).toBeInTheDocument();
    expect(screen.getByText("MP4")).toBeInTheDocument();
  });

  it("should extract and show hostname", () => {
    renderTable();
    expect(screen.getByText("example.com")).toBeInTheDocument();
    expect(screen.getByText("cdn.test.org")).toBeInTheDocument();
  });

  it("should select download on row click", async () => {
    const user = userEvent.setup();
    renderTable();
    const row = screen.getByText("file1.zip").closest("tr");
    if (row) await user.click(row);
    expect(useUiStore.getState().selectedDownloadId).toBe("1");
    expect(useUiStore.getState().selectedDownloadIds).toEqual(["1"]);
  });

  it("should show raw error details without selecting the row when the icon is clicked", async () => {
    const user = userEvent.setup();

    renderTable();

    await user.click(screen.getByRole("button", { name: "Show download error" }));

    expect(screen.getByText("tls handshake failed")).toBeInTheDocument();
    expect(useUiStore.getState().selectedDownloadId).toBeNull();
    expect(useUiStore.getState().selectedDownloadIds).toEqual([]);
  });

  describe("action button visibility per download state", () => {
    function makeDownload(overrides: Partial<DownloadView>): DownloadView {
      return {
        id: "99",
        fileName: "test-file.zip",
        url: "https://example.com/test-file.zip",
        sourceHostname: "example.com",
        state: "Downloading",
        progressPercent: 50,
        speedBytesPerSec: 1024,
        downloadedBytes: 5000,
        totalBytes: 10000,
        etaSeconds: 5,
        segmentsActive: 1,
        segmentsTotal: 2,
        moduleName: null,
        accountName: null,
        priority: 5,
        queuePosition: 0,
        createdAt: Date.now(),
        ...overrides,
      };
    }

    it("should show pause button for Downloading state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Downloading" })] });
      expect(container.querySelector(".lucide-pause")).toBeTruthy();
    });

    it("should not show pause button for Queued state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Queued" })] });
      expect(container.querySelector(".lucide-pause")).toBeNull();
    });

    it("should not show pause button for Retry state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Retry" })] });
      expect(container.querySelector(".lucide-pause")).toBeNull();
    });

    it("should not show retry button for Retry state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Retry" })] });
      expect(container.querySelector(".lucide-rotate-ccw")).toBeNull();
    });

    it("should show resume button for Paused state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Paused" })] });
      expect(container.querySelector(".lucide-play")).toBeTruthy();
    });

    it("should not show any action button for Completed state", () => {
      const { container } = renderTable({ downloads: [makeDownload({ state: "Completed" })] });
      expect(container.querySelector(".lucide-pause")).toBeNull();
      expect(container.querySelector(".lucide-play")).toBeNull();
      expect(container.querySelector(".lucide-rotate-ccw")).toBeNull();
    });
  });

  describe("open-file / open-folder menu items", () => {
    it("should expose Open file and Open folder menu items only for Completed rows", async () => {
      const user = userEvent.setup();
      renderTable();

      // Completed row (image.png) — menu must include both items
      const moreButtons = screen.getAllByLabelText("More actions");
      const completedButton = moreButtons[1];
      await user.click(completedButton);
      expect(screen.getByText("Open file")).toBeInTheDocument();
      expect(screen.getByText("Open folder")).toBeInTheDocument();
    });

    it("should hide Open file and Open folder items for non-completed rows", async () => {
      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[0]);
      expect(screen.queryByText("Open file")).not.toBeInTheDocument();
      expect(screen.queryByText("Open folder")).not.toBeInTheDocument();
    });

    it("should invoke download_open_file when Open file clicked", async () => {
      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[1]);
      await user.click(screen.getByText("Open file"));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith(
          "download_open_file",
          expect.objectContaining({ id: 2 }),
        );
      });
    });

    it("should invoke download_open_folder when Open folder clicked", async () => {
      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[1]);
      await user.click(screen.getByText("Open folder"));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith(
          "download_open_folder",
          expect.objectContaining({ id: 2 }),
        );
      });
    });

    it("should surface File not found toast when backend reports missing file", async () => {
      invokeMock.mockRejectedValueOnce(new Error("Not found: file not found: /tmp/x"));

      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[1]);
      await user.click(screen.getByText("Open file"));

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalledWith("File not found");
      });
    });
  });

  describe("re-download action", () => {
    it("exposes a Re-download menu item only on Completed rows", async () => {
      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[1]);
      expect(screen.getByText("Re-download")).toBeInTheDocument();
    });

    it("hides Re-download on non-completed rows", async () => {
      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[0]);
      expect(screen.queryByText("Re-download")).not.toBeInTheDocument();
    });

    it('invokes download_redownload with sourceKind "download" and the row id', async () => {
      invokeMock.mockResolvedValueOnce({ kind: "created", id: 77 });

      const user = userEvent.setup();
      renderTable();

      await user.click(screen.getAllByLabelText("More actions")[1]);
      await user.click(screen.getByText("Re-download"));

      await waitFor(() =>
        expect(invokeMock).toHaveBeenCalledWith(
          "download_redownload",
          expect.objectContaining({
            sourceKind: "download",
            sourceId: "2",
            overwriteMode: null,
          }),
        ),
      );
    });
  });

  describe("queue reorder", () => {
    const QUEUE_DOWNLOADS: DownloadView[] = [
      {
        id: "10",
        fileName: "first.zip",
        url: "https://example.com/first.zip",
        sourceHostname: "example.com",
        state: "Queued",
        progressPercent: 0,
        speedBytesPerSec: 0,
        downloadedBytes: 0,
        totalBytes: 1000,
        etaSeconds: null,
        segmentsActive: 0,
        segmentsTotal: 1,
        moduleName: null,
        accountName: null,
        priority: 5,
        queuePosition: 1,
        createdAt: Date.now(),
      },
      {
        id: "11",
        fileName: "second.zip",
        url: "https://example.com/second.zip",
        sourceHostname: "example.com",
        state: "Queued",
        progressPercent: 0,
        speedBytesPerSec: 0,
        downloadedBytes: 0,
        totalBytes: 2000,
        etaSeconds: null,
        segmentsActive: 0,
        segmentsTotal: 1,
        moduleName: null,
        accountName: null,
        priority: 5,
        queuePosition: 2,
        createdAt: Date.now(),
      },
      {
        id: "12",
        fileName: "third.zip",
        url: "https://example.com/third.zip",
        sourceHostname: "example.com",
        state: "Queued",
        progressPercent: 0,
        speedBytesPerSec: 0,
        downloadedBytes: 0,
        totalBytes: 3000,
        etaSeconds: null,
        segmentsActive: 0,
        segmentsTotal: 1,
        moduleName: null,
        accountName: null,
        priority: 5,
        queuePosition: 3,
        createdAt: Date.now(),
      },
    ];

    it("computes the new order when dragging second row onto first", () => {
      const result = computeReorderedIds(QUEUE_DOWNLOADS, "11", "10");
      expect(result).toEqual([11, 10, 12]);
    });

    it("returns null when dragging onto itself", () => {
      const result = computeReorderedIds(QUEUE_DOWNLOADS, "10", "10");
      expect(result).toBeNull();
    });

    it("filters non-reorderable downloads from the result", () => {
      const mixed: DownloadView[] = [
        ...QUEUE_DOWNLOADS,
        {
          ...QUEUE_DOWNLOADS[0],
          id: "13",
          fileName: "done.zip",
          state: "Completed",
        },
      ];
      const result = computeReorderedIds(mixed, "12", "10");
      expect(result).toEqual([12, 10, 11]);
      expect(result).not.toContain(13);
    });

    it("renders a drag handle for each reorderable row", () => {
      renderTable({ downloads: QUEUE_DOWNLOADS, filter: "queued" });
      const handles = screen.getAllByLabelText("Drag to reorder");
      expect(handles.length).toBe(QUEUE_DOWNLOADS.length);
    });

    it("invokes download_move_to_top when Move to top is clicked", async () => {
      const user = userEvent.setup();
      renderTable({ downloads: QUEUE_DOWNLOADS, filter: "queued" });

      await user.click(screen.getAllByLabelText("More actions")[0]);
      await user.click(screen.getByText("Move to top"));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith(
          "download_move_to_top",
          expect.objectContaining({ id: 10 }),
        );
      });
    });

    it("invokes download_move_to_bottom when Move to bottom is clicked", async () => {
      const user = userEvent.setup();
      renderTable({ downloads: QUEUE_DOWNLOADS, filter: "queued" });

      await user.click(screen.getAllByLabelText("More actions")[1]);
      await user.click(screen.getByText("Move to bottom"));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith(
          "download_move_to_bottom",
          expect.objectContaining({ id: 11 }),
        );
      });
    });
  });
});
