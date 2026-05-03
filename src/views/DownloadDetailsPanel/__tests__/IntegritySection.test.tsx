import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { IntegritySection } from "../IntegritySection";
import type { DownloadDetailView } from "@/types/download";

const { mockInvoke } = vi.hoisted(() => ({ mockInvoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

function makeDetail(overrides: Partial<DownloadDetailView> = {}): DownloadDetailView {
  return {
    id: "dl-1",
    fileName: "file.bin",
    url: "https://example.com/file.bin",
    sourceHostname: "example.com",
    state: "Completed",
    progressPercent: 100,
    speedBytesPerSec: 0,
    downloadedBytes: 100,
    totalBytes: 100,
    etaSeconds: null,
    segments: [],
    checksumExpected: null,
    checksumComputed: null,
    checksumAlgorithm: null,
    destinationPath: "/tmp/file.bin",
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 5,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function renderSection(detail: DownloadDetailView) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <IntegritySection download={detail} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("IntegritySection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should render dash placeholders when no expected checksum", () => {
    renderSection(makeDetail());
    const dashes = screen.getAllByText("—");
    expect(dashes.length).toBeGreaterThanOrEqual(3);
    expect(screen.queryByRole("button", { name: /verify/i })).not.toBeInTheDocument();
  });

  it("should display algorithm, expected and computed when both present", () => {
    renderSection(
      makeDetail({
        checksumExpected: "d41d8cd98f00b204e9800998ecf8427e",
        checksumComputed: "d41d8cd98f00b204e9800998ecf8427e",
        checksumAlgorithm: "MD5",
      }),
    );
    expect(screen.getByText("MD5")).toBeInTheDocument();
    expect(screen.getAllByText("d41d8cd98f00b204e9800998ecf8427e").length).toBeGreaterThanOrEqual(
      2,
    );
    expect(screen.getByTestId("checksum-status")).toHaveTextContent(/Match/);
  });

  it("should derive MD5 label from hex length when checksumAlgorithm is null", () => {
    // Migrated rows (created before checksum_algorithm column) only have
    // checksumExpected. Display must still show MD5 instead of guessing
    // SHA-256.
    renderSection(
      makeDetail({
        checksumExpected: "d41d8cd98f00b204e9800998ecf8427e",
        checksumAlgorithm: null,
      }),
    );
    expect(screen.getByText("MD5")).toBeInTheDocument();
  });

  it("should derive SHA-256 label from 64-char hex when checksumAlgorithm is null", () => {
    renderSection(
      makeDetail({
        checksumExpected: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        checksumAlgorithm: null,
      }),
    );
    expect(screen.getByText("SHA-256")).toBeInTheDocument();
  });

  it("should mark mismatch when computed differs from expected", () => {
    renderSection(
      makeDetail({
        checksumExpected: "d41d8cd98f00b204e9800998ecf8427e",
        checksumComputed: "00000000000000000000000000000000",
        checksumAlgorithm: "MD5",
      }),
    );
    expect(screen.getByTestId("checksum-status")).toHaveTextContent(/Mismatch/);
  });

  it("should call download_verify_checksum IPC and update status when user clicks Verify", async () => {
    mockInvoke.mockResolvedValueOnce("verified");
    renderSection(
      makeDetail({
        id: "42",
        checksumExpected: "d41d8cd98f00b204e9800998ecf8427e",
        checksumComputed: null,
        checksumAlgorithm: "MD5",
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /verify/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("download_verify_checksum", { id: 42 });
    });
    await waitFor(() => {
      expect(screen.getByTestId("checksum-status")).toHaveTextContent(/Match/);
    });
  });

  it("should surface backend error when verify command fails", async () => {
    mockInvoke.mockRejectedValueOnce("disk on fire");
    renderSection(
      makeDetail({
        checksumExpected: "d41d8cd98f00b204e9800998ecf8427e",
        checksumAlgorithm: "MD5",
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /verify/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("disk on fire");
    });
  });
});
