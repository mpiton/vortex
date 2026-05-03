import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { FileInfoSection } from "../FileInfoSection";
import type { DownloadDetailView } from "@/types/download";

const invokeMock = vi.fn().mockResolvedValue(null);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

beforeEach(() => {
  invokeMock.mockClear();
  invokeMock.mockResolvedValue(null);
});

function mockDownloadDetail(overrides?: Partial<DownloadDetailView>): DownloadDetailView {
  return {
    id: "dl-1",
    fileName: "test-file.zip",
    url: "https://example.com/test-file.zip",
    sourceHostname: "example.com",
    state: "Downloading",
    progressPercent: 50,
    speedBytesPerSec: 1048576,
    downloadedBytes: 524288,
    totalBytes: 1048576,
    etaSeconds: 30,
    segments: [],
    checksumExpected: null,
    checksumComputed: null,
    checksumAlgorithm: null,
    destinationPath: "/home/user/Downloads/test-file.zip",
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 5,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

function renderWithTooltip(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("FileInfoSection", () => {
  it("should display filename", () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail()} />);
    expect(screen.getAllByText("test-file.zip").length).toBeGreaterThan(0);
  });

  it("should display formatted file size", () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail({ totalBytes: 1048576 })} />);
    expect(screen.getByText("1.00 MB")).toBeInTheDocument();
  });

  it("should display destination path", () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail()} />);
    expect(screen.getAllByText("/home/user/Downloads/test-file.zip").length).toBeGreaterThan(0);
  });

  it("should show Open file and Open folder buttons for completed download", () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail({ state: "Completed" })} />);
    expect(screen.getByRole("button", { name: "Open file" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open folder" })).toBeInTheDocument();
  });

  it("should hide Open file and Open folder buttons while download is not completed", () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail({ state: "Downloading" })} />);
    expect(screen.queryByRole("button", { name: "Open file" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open folder" })).not.toBeInTheDocument();
  });

  it("should invoke download_open_file when Open file clicked", async () => {
    const user = userEvent.setup();
    renderWithTooltip(
      <FileInfoSection download={mockDownloadDetail({ id: "42", state: "Completed" })} />,
    );
    await user.click(screen.getByRole("button", { name: "Open file" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "download_open_file",
        expect.objectContaining({ id: 42 }),
      );
    });
  });

  it("should invoke download_open_folder when Open folder clicked", async () => {
    const user = userEvent.setup();
    renderWithTooltip(
      <FileInfoSection download={mockDownloadDetail({ id: "7", state: "Completed" })} />,
    );
    await user.click(screen.getByRole("button", { name: "Open folder" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "download_open_folder",
        expect.objectContaining({ id: 7 }),
      );
    });
  });
});
