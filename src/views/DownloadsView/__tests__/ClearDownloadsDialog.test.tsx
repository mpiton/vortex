import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ClearDownloadsDialog } from "../ClearDownloadsDialog";

function renderDialog(overrides: Partial<Parameters<typeof ClearDownloadsDialog>[0]> = {}) {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    targetState: "completed" as const,
    count: 3,
    onConfirm: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(<ClearDownloadsDialog {...props} />);
  return props;
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
});

describe("ClearDownloadsDialog", () => {
  it("renders the completed title with the provided count", () => {
    renderDialog({ targetState: "completed", count: 3 });
    expect(screen.getByText(/Clear 3 completed downloads\?/i)).toBeInTheDocument();
  });

  it("renders the failed title when targetState is error", () => {
    renderDialog({ targetState: "error", count: 2 });
    expect(screen.getByText(/Clear 2 failed downloads\?/i)).toBeInTheDocument();
  });

  it("does not show the warning panel by default", () => {
    renderDialog();
    expect(screen.queryByText(/Permanent deletion/i)).not.toBeInTheDocument();
  });

  it("reveals the warning panel when the checkbox is checked", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("checkbox", { name: /also delete files from disk/i }));
    expect(screen.getByText(/Permanent deletion/i)).toBeInTheDocument();
  });

  it("primary button label switches when the checkbox is checked", async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(screen.getByRole("button", { name: /^clear$/i })).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: /also delete files from disk/i }));
    expect(screen.getByRole("button", { name: /clear and delete files/i })).toBeInTheDocument();
  });

  it("calls onConfirm with deleteFiles:false when the box is not checked", async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole("button", { name: /^clear$/i }));
    expect(props.onConfirm).toHaveBeenCalledWith(false);
  });

  it("calls onConfirm with deleteFiles:true when the box is checked", async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole("checkbox", { name: /also delete files from disk/i }));
    await user.click(screen.getByRole("button", { name: /clear and delete files/i }));
    expect(props.onConfirm).toHaveBeenCalledWith(true);
  });

  it("calls onOpenChange(false) when cancel is clicked", async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });
});
