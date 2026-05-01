import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { DownloadView } from "@/types/download";
import type { PackageView } from "@/types/package";
import { PackagesView } from "../PackagesView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);
const mockOpenDialog = vi.mocked(openDialog);
const mockToastSuccess = vi.mocked(toast.success);
const mockToastError = vi.mocked(toast.error);

function samplePackages(): PackageView[] {
  return [
    {
      id: "pkg-1",
      name: "Holiday playlist",
      sourceType: "playlist",
      folderPath: "/srv/dl/holiday",
      autoExtract: false,
      priority: 5,
      createdAt: 1_700_000_000_000,
      downloadsCount: 3,
      totalBytes: 30_000_000,
      downloadedBytes: 15_000_000,
      progressPercent: 50,
      allCompleted: false,
    },
    {
      id: "pkg-2",
      name: "Backup archive",
      sourceType: "split_archive",
      folderPath: null,
      autoExtract: true,
      priority: 7,
      createdAt: 1_700_000_001_000,
      downloadsCount: 0,
      totalBytes: 0,
      downloadedBytes: 0,
      progressPercent: 0,
      allCompleted: true,
    },
  ];
}

function sampleChildren(): DownloadView[] {
  return [
    {
      id: "42",
      fileName: "song-01.mp3",
      url: "https://example.com/song-01.mp3",
      sourceHostname: "example.com",
      state: "Downloading",
      progressPercent: 60,
      speedBytesPerSec: 100_000,
      downloadedBytes: 6_000_000,
      totalBytes: 10_000_000,
      etaSeconds: 40,
      segmentsActive: 4,
      segmentsTotal: 4,
      moduleName: "youtube",
      accountName: null,
      priority: 5,
      queuePosition: 1,
      createdAt: 1_700_000_002_000,
    },
    {
      id: "43",
      fileName: "song-02.mp3",
      url: "https://example.com/song-02.mp3",
      sourceHostname: "example.com",
      state: "Paused",
      progressPercent: 20,
      speedBytesPerSec: 0,
      downloadedBytes: 2_000_000,
      totalBytes: 10_000_000,
      etaSeconds: null,
      segmentsActive: 0,
      segmentsTotal: 4,
      moduleName: "youtube",
      accountName: null,
      priority: 5,
      queuePosition: 2,
      createdAt: 1_700_000_003_000,
    },
  ];
}

function renderView() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  render(
    <QueryClientProvider client={client}>
      <PackagesView />
    </QueryClientProvider>,
  );
  return { client };
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
  mockInvoke.mockReset();
  mockOpenDialog.mockReset();
  mockToastSuccess.mockClear();
  mockToastError.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

function defaultInvokeImpl() {
  return async (command: string, _args?: unknown) => {
    if (command === "package_list") return samplePackages();
    if (command === "package_list_downloads") return sampleChildren();
    return null;
  };
}

describe("PackagesView", () => {
  it("renders packages returned by package_list", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    renderView();
    await waitFor(() => {
      expect(screen.getByText("Holiday playlist")).toBeInTheDocument();
      expect(screen.getByText("Backup archive")).toBeInTheDocument();
    });
    expect(screen.queryByText(/coming soon/i)).not.toBeInTheDocument();
  });

  it("renders empty state when no packages exist", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return [];
      return null;
    });
    renderView();
    await waitFor(() => {
      expect(screen.getByTestId("packages-empty")).toBeInTheDocument();
    });
  });

  it("expands a package and lists its downloads", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await waitFor(() => {
      expect(screen.getByText("song-01.mp3")).toBeInTheDocument();
      expect(screen.getByText("song-02.mp3")).toBeInTheDocument();
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      "package_list_downloads",
      expect.objectContaining({ id: "pkg-1" }),
    );
  });

  it("filters by source type via filter chips", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("packages-filter-playlist"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_list",
        expect.objectContaining({ sourceType: "playlist" }),
      );
    });
  });

  it("debounces search and forwards nameQ to package_list", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    renderView();
    await screen.findByText("Holiday playlist");
    const input = screen.getByTestId("packages-search") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "holi" } });
    await waitFor(
      () => {
        expect(mockInvoke).toHaveBeenCalledWith(
          "package_list",
          expect.objectContaining({ nameQ: "holi" }),
        );
      },
      { timeout: 2000 },
    );
  });

  it("creates a package via the New package dialog", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_create") return "pkg-99";
      if (command === "package_list_downloads") return [];
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("packages-add-trigger"));
    await user.type(screen.getByTestId("package-add-name"), "New box");
    await user.click(screen.getByTestId("package-add-submit"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_create",
        expect.objectContaining({ name: "New box", sourceType: "manual" }),
      );
    });
    expect(mockToastSuccess).toHaveBeenCalled();
  });

  it("renames a package via inline rename dialog", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_update") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-rename"));
    const input = screen.getByTestId("package-rename-input");
    await user.clear(input);
    await user.type(input, "Renamed pkg");
    await user.click(screen.getByTestId("package-rename-submit"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_update",
        expect.objectContaining({
          id: "pkg-1",
          patch: expect.objectContaining({ name: "Renamed pkg" }),
        }),
      );
    });
  });

  it("toggles auto-extract via switch", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_toggle_auto_extract") return true;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-auto-extract"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_toggle_auto_extract",
        expect.objectContaining({ id: "pkg-1" }),
      );
    });
  });

  it("changes priority via select", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_set_priority") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    const select = screen.getByTestId("package-row-pkg-1-priority") as HTMLSelectElement;
    await user.selectOptions(select, "9");
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_set_priority",
        expect.objectContaining({ id: "pkg-1", priority: 9 }),
      );
    });
  });

  it("sets a password via the password dialog (masked)", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_set_password") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-password"));
    const input = screen.getByTestId("package-password-input");
    expect(input).toHaveAttribute("type", "password");
    await user.type(input, "topsecret");
    await user.click(screen.getByTestId("package-password-submit"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_set_password",
        expect.objectContaining({ id: "pkg-1", password: "topsecret" }),
      );
    });
  });

  it("opens the change folder dialog and persists the new path", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_move_to_folder") return { moved: [42, 43], failed: [] };
      return null;
    });
    mockOpenDialog.mockResolvedValue("/srv/dl/new");
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-folder"));
    await user.click(screen.getByTestId("package-folder-browse"));
    await waitFor(() => {
      expect((screen.getByTestId("package-folder-input") as HTMLInputElement).value).toBe(
        "/srv/dl/new",
      );
    });
    await user.click(screen.getByTestId("package-folder-submit"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_move_to_folder",
        expect.objectContaining({ id: "pkg-1", newFolder: "/srv/dl/new" }),
      );
    });
  });

  it("deletes a package after confirmation", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_delete") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-delete"));
    await user.click(screen.getByTestId("package-delete-confirm"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_delete",
        expect.objectContaining({ id: "pkg-1", deleteDownloads: false }),
      );
    });
  });

  it("pauses every download in a package via Pause all", async () => {
    mockInvoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_list_downloads") return sampleChildren();
      if (command === "download_pause") {
        const id = (args as { id: number }).id;
        expect([42, 43]).toContain(id);
        return null;
      }
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");
    await user.click(screen.getByTestId("package-row-pkg-1-pause-all"));
    await waitFor(() => {
      expect(
        mockInvoke.mock.calls.filter(([c]) => c === "download_pause"),
      ).toHaveLength(2);
    });
  });

  it("moves a download between packages via drag and drop", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_list_downloads") return sampleChildren();
      if (command === "package_remove_download") return null;
      if (command === "package_add_download") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    const draggable = screen.getByTestId("package-download-row-42");
    const dropZone = screen.getByTestId("package-row-pkg-2-dropzone");
    const dataTransfer = {
      data: {} as Record<string, string>,
      setData(key: string, value: string) {
        this.data[key] = value;
      },
      getData(key: string) {
        return this.data[key] ?? "";
      },
    };
    fireEvent.dragStart(draggable, { dataTransfer });
    fireEvent.dragOver(dropZone, { dataTransfer });
    fireEvent.drop(dropZone, { dataTransfer });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_remove_download",
        expect.objectContaining({ packageId: "pkg-1", downloadId: 42 }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_add_download",
        expect.objectContaining({ packageId: "pkg-2", downloadId: 42 }),
      );
    });
  });

  it("marks a download as pending-move when the Move button is pressed", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    const moveButton = screen.getByTestId("package-download-row-42-move");
    expect(moveButton).toHaveAttribute("aria-pressed", "false");

    await user.click(moveButton);

    const cancelButton = screen.getByTestId("package-download-row-42-move-cancel");
    expect(cancelButton).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("packages-move-live-region")).toHaveTextContent(
      /selected song-01\.mp3/i,
    );
  });

  it("shows Move-here target on non-source packages and hides it on the source", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    expect(screen.queryByTestId("package-row-pkg-1-move-target")).not.toBeInTheDocument();
    expect(screen.queryByTestId("package-row-pkg-2-move-target")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("package-download-row-42-move"));

    expect(screen.queryByTestId("package-row-pkg-1-move-target")).not.toBeInTheDocument();
    expect(screen.getByTestId("package-row-pkg-2-move-target")).toBeInTheDocument();
  });

  it("executes a keyboard move via Move-here target and announces success", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_list_downloads") return sampleChildren();
      if (command === "package_remove_download") return null;
      if (command === "package_add_download") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    await user.click(screen.getByTestId("package-download-row-42-move"));
    await user.click(screen.getByTestId("package-row-pkg-2-move-target"));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_remove_download",
        expect.objectContaining({ packageId: "pkg-1", downloadId: 42 }),
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "package_add_download",
        expect.objectContaining({ packageId: "pkg-2", downloadId: 42 }),
      );
    });
    await waitFor(() => {
      expect(screen.getByTestId("packages-move-live-region")).toHaveTextContent(
        /song-01\.mp3 moved to backup archive/i,
      );
    });
    expect(mockToastSuccess).toHaveBeenCalled();
    expect(
      screen.queryByTestId("package-row-pkg-2-move-target"),
    ).not.toBeInTheDocument();
  });

  it("cancels a pending move and clears the live region", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    await user.click(screen.getByTestId("package-download-row-42-move"));
    expect(
      screen.getByTestId("package-download-row-42-move-cancel"),
    ).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByTestId("package-download-row-42-move-cancel"));

    expect(
      screen.getByTestId("package-download-row-42-move"),
    ).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("packages-move-live-region")).toHaveTextContent(
      /move cancelled/i,
    );
    expect(
      screen.queryByTestId("package-row-pkg-2-move-target"),
    ).not.toBeInTheDocument();
  });

  it("announces an error when keyboard move fails on add (rollback succeeds)", async () => {
    mockInvoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_list_downloads") return sampleChildren();
      if (command === "package_remove_download") return null;
      if (command === "package_add_download") {
        const { packageId } = args as { packageId: string };
        if (packageId === "pkg-2") throw new Error("boom");
        return null;
      }
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    await user.click(screen.getByTestId("package-download-row-42-move"));
    await user.click(screen.getByTestId("package-row-pkg-2-move-target"));

    await waitFor(() => {
      expect(screen.getByTestId("packages-move-live-region")).toHaveTextContent(
        /failed to move song-01\.mp3/i,
      );
    });
    expect(mockToastError).toHaveBeenCalled();
    const addCalls = mockInvoke.mock.calls.filter(
      ([c]) => c === "package_add_download",
    );
    expect(addCalls).toHaveLength(2);
    expect(addCalls[0]?.[1]).toMatchObject({ packageId: "pkg-2", downloadId: 42 });
    expect(addCalls[1]?.[1]).toMatchObject({ packageId: "pkg-1", downloadId: 42 });
  });

  it("does not clobber a newer pending-move when a previous executeMove resolves", async () => {
    const removeGate: { resolve: ((value: unknown) => void) | null } = {
      resolve: null,
    };
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") return samplePackages();
      if (command === "package_list_downloads") return sampleChildren();
      if (command === "package_remove_download") {
        return new Promise((res) => {
          removeGate.resolve = res;
        });
      }
      if (command === "package_add_download") return null;
      return null;
    });
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Holiday playlist");
    await user.click(screen.getByTestId("package-row-pkg-1-toggle"));
    await screen.findByText("song-01.mp3");

    await user.click(screen.getByTestId("package-download-row-42-move"));
    await user.click(screen.getByTestId("package-row-pkg-2-move-target"));

    await user.click(screen.getByTestId("package-download-row-43-move"));
    expect(
      screen.getByTestId("package-download-row-43-move-cancel"),
    ).toHaveAttribute("aria-pressed", "true");

    removeGate.resolve?.(null);

    await waitFor(() => {
      expect(
        screen.getByTestId("package-download-row-43-move-cancel"),
      ).toHaveAttribute("aria-pressed", "true");
    });
  });

  it("surfaces the error state when package_list fails", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "package_list") throw new Error("boom");
      return null;
    });
    renderView();
    await waitFor(() => {
      expect(screen.getByTestId("packages-error")).toHaveTextContent(/boom/i);
    });
  });

  it("shows count of files per package", async () => {
    mockInvoke.mockImplementation(defaultInvokeImpl());
    renderView();
    await screen.findByText("Holiday playlist");
    const row = screen.getByTestId("package-row-pkg-1");
    expect(within(row).getByText(/3 files/i)).toBeInTheDocument();
  });
});
