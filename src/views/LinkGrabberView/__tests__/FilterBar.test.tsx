import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { FilterBar } from "../FilterBar";

describe("FilterBar", () => {
  it("should render all 4 filter tabs", () => {
    render(<FilterBar activeFilter="all" onFilterChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "All" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Online" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Offline" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Media" })).toBeInTheDocument();
  });

  it("should apply default variant to active filter and outline to others", () => {
    render(<FilterBar activeFilter="online" onFilterChange={vi.fn()} />);
    const onlineBtn = screen.getByRole("button", { name: "Online" });
    const allBtn = screen.getByRole("button", { name: "All" });
    // Active button has bg-primary (default variant), inactive has border (outline variant)
    expect(onlineBtn.className).toContain("bg-primary");
    expect(allBtn.className).toContain("border");
  });

  it("should call onFilterChange with correct FilterType when tab is clicked", async () => {
    const user = userEvent.setup();
    const onFilterChange = vi.fn();
    render(<FilterBar activeFilter="all" onFilterChange={onFilterChange} />);

    await user.click(screen.getByRole("button", { name: "Offline" }));
    expect(onFilterChange).toHaveBeenCalledOnce();
    expect(onFilterChange).toHaveBeenCalledWith("offline");
  });

  it("should call onFilterChange with 'media' when Media tab is clicked", async () => {
    const user = userEvent.setup();
    const onFilterChange = vi.fn();
    render(<FilterBar activeFilter="all" onFilterChange={onFilterChange} />);

    await user.click(screen.getByRole("button", { name: "Media" }));
    expect(onFilterChange).toHaveBeenCalledWith("media");
  });
});
