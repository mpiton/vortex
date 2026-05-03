import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MoveDialog, deriveDefaultDir } from "../MoveDialog";

const browseFolderMock = vi.fn<(defaultPath?: string | null) => Promise<string | null>>();

vi.mock("@/hooks/useBrowseFolder", () => ({
  useBrowseFolder: () => browseFolderMock,
}));

function renderDialog(overrides: Partial<Parameters<typeof MoveDialog>[0]> = {}) {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    count: 2,
    currentPath: "/old/folder/file.bin",
    onConfirm: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(<MoveDialog {...props} />);
  return props;
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
  browseFolderMock.mockReset();
});

describe("MoveDialog", () => {
  it("renders the title with the count of selected downloads", () => {
    renderDialog({ count: 2 });
    expect(screen.getByText(/Move 2 downloads/i)).toBeInTheDocument();
  });

  it("renders the singular title when only one download is selected", () => {
    renderDialog({ count: 1 });
    expect(screen.getByText(/Move 1 download$/i)).toBeInTheDocument();
  });

  it("shows the current path when provided", () => {
    renderDialog({ currentPath: "/some/old/path.bin" });
    expect(screen.getByTestId("move-current-path")).toHaveTextContent("/some/old/path.bin");
  });

  it("hides the current-path block when no path is supplied", () => {
    renderDialog({ currentPath: undefined });
    expect(screen.queryByTestId("move-current-path")).not.toBeInTheDocument();
  });

  it("starts with no destination selected and confirm disabled", () => {
    renderDialog();
    expect(screen.getByTestId("move-destination-path")).toHaveTextContent(/no folder selected/i);
    expect(screen.getByRole("button", { name: /^move$/i })).toBeDisabled();
  });

  it("calls the folder picker with the parent directory of the current path", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue("/picked/folder");
    renderDialog({ currentPath: "/old/folder/file.bin" });

    await user.click(screen.getByRole("button", { name: /browse/i }));

    expect(browseFolderMock).toHaveBeenCalledWith("/old/folder");
  });

  it("falls back to null defaultPath when no current path is provided", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue("/picked");
    renderDialog({ currentPath: undefined });

    await user.click(screen.getByRole("button", { name: /browse/i }));

    expect(browseFolderMock).toHaveBeenCalledWith(null);
  });

  it("displays the selected path and enables confirm after picking a folder", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue("/new/destination");
    renderDialog();

    await user.click(screen.getByRole("button", { name: /browse/i }));

    expect(screen.getByTestId("move-destination-path")).toHaveTextContent("/new/destination");
    expect(screen.getByRole("button", { name: /^move$/i })).toBeEnabled();
  });

  it("ignores cancellation of the OS picker (confirm stays disabled)", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue(null);
    renderDialog();

    await user.click(screen.getByRole("button", { name: /browse/i }));

    expect(screen.getByRole("button", { name: /^move$/i })).toBeDisabled();
    expect(screen.getByTestId("move-destination-path")).toHaveTextContent(/no folder selected/i);
  });

  it("calls onConfirm with the picked path and closes the dialog on success", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue("/dest");
    const props = renderDialog();

    await user.click(screen.getByRole("button", { name: /browse/i }));
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    expect(props.onConfirm).toHaveBeenCalledWith("/dest");
    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the dialog open and the picked path visible when onConfirm rejects", async () => {
    const user = userEvent.setup();
    browseFolderMock.mockResolvedValue("/dest");
    const props = renderDialog({
      onConfirm: vi.fn().mockRejectedValue(new Error("backend down")),
    });

    await user.click(screen.getByRole("button", { name: /browse/i }));
    await user.click(screen.getByRole("button", { name: /^move$/i }));

    expect(props.onConfirm).toHaveBeenCalledWith("/dest");
    // Dialog must NOT receive an onOpenChange(false) so the user can retry.
    expect(props.onOpenChange).not.toHaveBeenCalledWith(false);
    expect(screen.getByTestId("move-destination-path")).toHaveTextContent("/dest");
  });

  it("calls onOpenChange(false) when cancel is clicked", async () => {
    const user = userEvent.setup();
    const props = renderDialog();

    await user.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("deriveDefaultDir", () => {
  it("returns null when no current path is supplied", () => {
    expect(deriveDefaultDir(undefined)).toBeNull();
  });

  it("strips the basename for a regular POSIX path", () => {
    expect(deriveDefaultDir("/old/folder/file.bin")).toBe("/old/folder");
  });

  it("returns the root for a POSIX path with the file at root", () => {
    // Without the root-aware branch this would return "" and the picker
    // would treat it as null, ignoring the user's actual current location.
    expect(deriveDefaultDir("/file.bin")).toBe("/");
  });

  it("preserves the trailing slash for a Windows drive root", () => {
    // "C:" alone resolves to the cwd of the C: drive on Windows; the picker
    // needs the trailing backslash to land on the actual drive root.
    expect(deriveDefaultDir("C:\\file.bin")).toBe("C:\\");
  });

  it("strips the basename for a nested Windows path", () => {
    expect(deriveDefaultDir("C:\\Users\\me\\file.bin")).toBe("C:\\Users\\me");
  });
});
