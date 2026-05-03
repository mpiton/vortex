import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FilterBar } from "../FilterBar";

const mockCounts: Record<string, number> = {
  Downloading: 3,
  Queued: 5,
  Completed: 12,
  Error: 2,
  Retry: 1,
  Paused: 4,
};

describe("FilterBar", () => {
  it("should render all filter tabs", () => {
    render(<FilterBar activeFilter="all" onFilterChange={vi.fn()} counts={mockCounts} />);
    expect(screen.getByText("All")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("Queued")).toBeInTheDocument();
    expect(screen.getByText("Done")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("should show correct count for Active (Downloading + Queued)", () => {
    render(<FilterBar activeFilter="all" onFilterChange={vi.fn()} counts={mockCounts} />);
    const activeButton = screen.getByText("Active").closest("button");
    expect(activeButton).toHaveTextContent("8");
  });

  it("should show correct count for Failed (Error + Retry)", () => {
    render(<FilterBar activeFilter="all" onFilterChange={vi.fn()} counts={mockCounts} />);
    const failedButton = screen.getByText("Failed").closest("button");
    expect(failedButton).toHaveTextContent("3");
  });

  it("should call onFilterChange when tab is clicked", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<FilterBar activeFilter="all" onFilterChange={onChange} counts={mockCounts} />);
    await user.click(screen.getByText("Active"));
    expect(onChange).toHaveBeenCalledWith("active");
  });

  it("should handle undefined counts gracefully", () => {
    render(<FilterBar activeFilter="all" onFilterChange={vi.fn()} counts={undefined} />);
    expect(screen.getByText("All")).toBeInTheDocument();
  });
});
