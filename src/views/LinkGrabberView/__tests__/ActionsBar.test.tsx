import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { ActionsBar } from "../ActionsBar";

describe("ActionsBar", () => {
  const defaultProps = {
    selectedCount: 0,
    totalCount: 10,
    onStartSelected: vi.fn(),
    onStartAll: vi.fn(),
    onClearAll: vi.fn(),
    onSelectAll: vi.fn(),
  };

  it("should show Select All with total count", () => {
    render(<ActionsBar {...defaultProps} totalCount={10} />);
    expect(screen.getByRole("button", { name: "Select All (10)" })).toBeInTheDocument();
  });

  it("should hide Start Selected when selectedCount is 0", () => {
    render(<ActionsBar {...defaultProps} selectedCount={0} />);
    expect(screen.queryByRole("button", { name: /Start Selected/ })).not.toBeInTheDocument();
  });

  it("should show Start Selected when selectedCount > 0", () => {
    render(<ActionsBar {...defaultProps} selectedCount={3} />);
    expect(screen.getByRole("button", { name: "Start Selected (3)" })).toBeInTheDocument();
  });

  it("should call onStartAll when Start All Online is clicked", async () => {
    const user = userEvent.setup();
    const onStartAll = vi.fn();
    render(<ActionsBar {...defaultProps} onStartAll={onStartAll} />);

    await user.click(screen.getByRole("button", { name: "Start All Online" }));
    expect(onStartAll).toHaveBeenCalledOnce();
  });

  it("should call onClearAll when Clear is clicked", async () => {
    const user = userEvent.setup();
    const onClearAll = vi.fn();
    render(<ActionsBar {...defaultProps} onClearAll={onClearAll} />);

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClearAll).toHaveBeenCalledOnce();
  });
});
