import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useUiStore } from "@/stores/uiStore";
import { ActionsBar } from "../ActionsBar";
import { downloadQueries } from "@/api/queries";

const { invokeMock, toastMock, browseFolderMock, useDownloadDetailMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  toastMock: { success: vi.fn(), error: vi.fn() },
  browseFolderMock: vi.fn<(p?: string | null) => Promise<string | null>>(),
  useDownloadDetailMock: vi.fn(() => ({ data: undefined })),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@/lib/toast", () => ({ toast: toastMock }));

// Mock the folder picker hook so the move dialog tests don't have to
// thread mockResolvedValueOnce through both the browse_folder and the
// download_change_directory_bulk invoke calls. Keeps the test focused on
// what the action bar does once a folder has been chosen.
vi.mock("@/hooks/useBrowseFolder", () => ({
  useBrowseFolder: () => browseFolderMock,
}));

// Stub the detail hook so the bar can read currentPath for the move dialog
// without firing the real `download_detail` IPC during tests.
vi.mock("@/hooks/useDownloadDetail", () => ({
  useDownloadDetail: () => useDownloadDetailMock(),
}));

function makeClient(counts?: Record<string, number>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  if (counts) {
    queryClient.setQueryData(downloadQueries.countByState(), counts);
  }
  return queryClient;
}

function renderBar(counts?: Record<string, number>) {
  const queryClient = makeClient(counts);
  return render(
    <QueryClientProvider client={queryClient}>
      <ActionsBar />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState({ selectedDownloadIds: [], selectedDownloadId: null });
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  toastMock.success.mockReset();
  toastMock.error.mockReset();
  browseFolderMock.mockReset();
  browseFolderMock.mockResolvedValue(null);
  useDownloadDetailMock.mockReset();
  useDownloadDetailMock.mockReturnValue({ data: undefined });
  window.localStorage.setItem("i18nextLng", "en");
});

describe("ActionsBar", () => {
  it("should show Pause All and Resume All when no selection", () => {
    renderBar();
    expect(screen.getByText("Pause All")).toBeInTheDocument();
    expect(screen.getByText("Resume All")).toBeInTheDocument();
  });

  it("should show selection count and actions when items selected", () => {
    useUiStore.setState({ selectedDownloadIds: ["1", "2", "3"] });
    renderBar();
    expect(screen.getByText("3 selected")).toBeInTheDocument();
    expect(screen.getByText("Cancel Selected")).toBeInTheDocument();
    expect(screen.getByText("Clear")).toBeInTheDocument();
  });

  it("should clear selection when Clear is clicked", async () => {
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ["1", "2"] });
    renderBar();
    await user.click(screen.getByText("Clear"));
    expect(useUiStore.getState().selectedDownloadIds).toEqual([]);
  });

  it("should use the singular French label when one item is selected", () => {
    window.localStorage.setItem("i18nextLng", "fr");
    useUiStore.setState({ selectedDownloadIds: ["1"] });
    renderBar();
    expect(screen.getByText("1 sélectionné")).toBeInTheDocument();
  });
});

describe("ActionsBar — clear completed/failed", () => {
  it('disables "Clear completed" when Completed count is 0', () => {
    renderBar({ Completed: 0, Error: 3 });
    expect(screen.getByRole("button", { name: /clear completed/i })).toBeDisabled();
  });

  it('disables "Clear failed" when Error count is 0', () => {
    renderBar({ Completed: 1, Error: 0 });
    expect(screen.getByRole("button", { name: /clear failed/i })).toBeDisabled();
  });

  it("invokes download_clear_completed with deleteFiles:false and shows success toast", async () => {
    invokeMock.mockResolvedValueOnce(3);
    const user = userEvent.setup();
    renderBar({ Completed: 3, Error: 0 });

    await user.click(screen.getByRole("button", { name: /clear completed/i }));
    await user.click(await screen.findByRole("button", { name: /^clear$/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("download_clear_completed", {
        deleteFiles: false,
      });
    });
    await waitFor(() => {
      expect(toastMock.success).toHaveBeenCalledWith(expect.stringContaining("3"));
    });
  });

  it("invokes download_clear_failed with deleteFiles:true when checkbox checked", async () => {
    invokeMock.mockResolvedValueOnce(2);
    const user = userEvent.setup();
    renderBar({ Completed: 0, Error: 2 });

    await user.click(screen.getByRole("button", { name: /clear failed/i }));
    await user.click(await screen.findByRole("checkbox", { name: /also delete files from disk/i }));
    await user.click(screen.getByRole("button", { name: /clear and delete files/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("download_clear_failed", {
        deleteFiles: true,
      });
    });
  });

  it("shows error toast when the mutation rejects", async () => {
    invokeMock.mockRejectedValueOnce(new Error("boom"));
    const user = userEvent.setup();
    renderBar({ Completed: 1, Error: 0 });

    await user.click(screen.getByRole("button", { name: /clear completed/i }));
    await user.click(await screen.findByRole("button", { name: /^clear$/i }));

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("boom"));
    });
  });
});

describe("ActionsBar — move selected", () => {
  it("hides the Move button when no items are selected", () => {
    renderBar();
    expect(screen.queryByRole("button", { name: /move to/i })).not.toBeInTheDocument();
  });

  it("shows the Move button when items are selected", () => {
    useUiStore.setState({ selectedDownloadIds: ["1", "2"] });
    renderBar();
    expect(screen.getByRole("button", { name: /move to/i })).toBeInTheDocument();
  });

  it("opens the move dialog when the Move button is clicked", async () => {
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ["1", "2"] });
    renderBar();

    await user.click(screen.getByRole("button", { name: /move to/i }));
    expect(screen.getByText(/Move 2 downloads/i)).toBeInTheDocument();
  });

  it("invokes the bulk IPC and clears selection on full success", async () => {
    browseFolderMock.mockResolvedValueOnce("/picked/folder");
    invokeMock.mockResolvedValueOnce({ moved: [1, 2], failed: [] });
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ["1", "2"] });
    // Seed the count-by-state cache so the action bar's auto-fetch
    // doesn't consume the first mocked invoke result intended for the
    // bulk move call.
    renderBar({ Completed: 0, Error: 0 });

    await user.click(screen.getByRole("button", { name: /move to/i }));
    await user.click(screen.getByRole("button", { name: /browse/i }));
    // Wait for the picked path to propagate so the dialog's primary
    // button is enabled before we try to click it.
    await waitFor(() => expect(screen.getByRole("button", { name: /^move$/i })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("download_change_directory_bulk", {
        ids: [1, 2],
        newDestinationDir: "/picked/folder",
      });
    });
    await waitFor(() => {
      expect(toastMock.success).toHaveBeenCalledWith(expect.stringContaining("2"));
    });
    expect(useUiStore.getState().selectedDownloadIds).toEqual([]);
  });

  it("keeps failed rows selected and shows a partial-failure toast", async () => {
    browseFolderMock.mockResolvedValueOnce("/dest");
    invokeMock.mockResolvedValueOnce({
      moved: [1],
      failed: [{ id: 2, message: "boom" }],
    });
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ["1", "2"] });
    renderBar({ Completed: 0, Error: 0 });

    await user.click(screen.getByRole("button", { name: /move to/i }));
    await user.click(screen.getByRole("button", { name: /browse/i }));
    await waitFor(() => expect(screen.getByRole("button", { name: /^move$/i })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalled();
    });
    // Selection ids are strings in the UI store; the IPC outcome surfaces
    // them as numbers, so the bar must coerce back to string when keeping
    // failed rows selected.
    expect(useUiStore.getState().selectedDownloadIds).toEqual(["2"]);
  });

  it("re-anchors selectedDownloadId to a failed row when the focused row was moved", async () => {
    browseFolderMock.mockResolvedValueOnce("/dest");
    invokeMock.mockResolvedValueOnce({
      moved: [1],
      failed: [{ id: 2, message: "boom" }],
    });
    const user = userEvent.setup();
    // Details panel is currently focused on the row that will succeed; after
    // the partial move it must re-anchor onto the failed row instead of
    // staying on a download that's no longer in the multi-select set.
    useUiStore.setState({
      selectedDownloadIds: ["1", "2"],
      selectedDownloadId: "1",
    });
    renderBar({ Completed: 0, Error: 0 });

    await user.click(screen.getByRole("button", { name: /move to/i }));
    await user.click(screen.getByRole("button", { name: /browse/i }));
    await waitFor(() => expect(screen.getByRole("button", { name: /^move$/i })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    await waitFor(() => expect(useUiStore.getState().selectedDownloadIds).toEqual(["2"]));
    expect(useUiStore.getState().selectedDownloadId).toBe("2");
  });

  it("keeps selectedDownloadId untouched when the focused row stays in the failed set", async () => {
    browseFolderMock.mockResolvedValueOnce("/dest");
    invokeMock.mockResolvedValueOnce({
      moved: [1],
      failed: [{ id: 2, message: "boom" }],
    });
    const user = userEvent.setup();
    useUiStore.setState({
      selectedDownloadIds: ["1", "2"],
      selectedDownloadId: "2",
    });
    renderBar({ Completed: 0, Error: 0 });

    await user.click(screen.getByRole("button", { name: /move to/i }));
    await user.click(screen.getByRole("button", { name: /browse/i }));
    await waitFor(() => expect(screen.getByRole("button", { name: /^move$/i })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    await waitFor(() => expect(useUiStore.getState().selectedDownloadIds).toEqual(["2"]));
    expect(useUiStore.getState().selectedDownloadId).toBe("2");
  });
});
