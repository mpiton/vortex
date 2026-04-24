import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { OverwriteDialog, type OverwriteDecision } from "../ui/OverwriteDialog";

function renderDialog(overrides: Partial<Parameters<typeof OverwriteDialog>[0]> = {}) {
  const onDecision = vi.fn<(d: OverwriteDecision) => void>();
  const onOpenChange = vi.fn<(open: boolean) => void>();
  const props = {
    open: true,
    onOpenChange,
    originalPath: "/downloads/report.pdf",
    suggestedPath: "/downloads/report (1).pdf",
    onDecision,
    ...overrides,
  };
  render(<OverwriteDialog {...props} />);
  return props;
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
});

describe("OverwriteDialog", () => {
  it("renders the original path in the description", () => {
    renderDialog();
    expect(screen.getByText(/\/downloads\/report\.pdf already exists/i)).toBeInTheDocument();
  });

  it("shows the suggested rename path", () => {
    renderDialog();
    expect(screen.getByTestId("overwrite-suggested-path")).toHaveTextContent(
      "/downloads/report (1).pdf",
    );
  });

  it('calls onDecision("overwrite") and closes when Overwrite is clicked', async () => {
    const user = userEvent.setup();
    const { onDecision, onOpenChange } = renderDialog();
    await user.click(screen.getByRole("button", { name: /overwrite/i }));
    expect(onDecision).toHaveBeenCalledWith("overwrite");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('calls onDecision("rename") when Keep both is clicked', async () => {
    const user = userEvent.setup();
    const { onDecision } = renderDialog();
    await user.click(screen.getByRole("button", { name: /keep both/i }));
    expect(onDecision).toHaveBeenCalledWith("rename");
  });

  it('calls onDecision("cancel") when Cancel is clicked', async () => {
    const user = userEvent.setup();
    const { onDecision } = renderDialog();
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));
    expect(onDecision).toHaveBeenCalledWith("cancel");
  });
});
