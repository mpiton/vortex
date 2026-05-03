import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PeriodSelector } from "../PeriodSelector";
import type { StatsPeriod } from "../derive";

const labels: Record<StatsPeriod, string> = {
  "7d": "7d",
  "30d": "30d",
  all: "All",
};

describe("PeriodSelector", () => {
  it("moves selection right on ArrowRight", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PeriodSelector value="7d" onChange={onChange} ariaLabel="period" labels={labels} />);

    await user.tab();
    expect(screen.getByRole("tab", { name: "7d" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenCalledWith("30d");
  });

  it("wraps from last to first on ArrowRight", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PeriodSelector value="all" onChange={onChange} ariaLabel="period" labels={labels} />);

    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenCalledWith("7d");
  });

  it("moves selection left on ArrowLeft", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PeriodSelector value="30d" onChange={onChange} ariaLabel="period" labels={labels} />);

    await user.tab();
    await user.keyboard("{ArrowLeft}");
    expect(onChange).toHaveBeenCalledWith("7d");
  });

  it("jumps to first/last with Home/End", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<PeriodSelector value="30d" onChange={onChange} ariaLabel="period" labels={labels} />);

    await user.tab();
    await user.keyboard("{Home}");
    expect(onChange).toHaveBeenCalledWith("7d");
    await user.keyboard("{End}");
    expect(onChange).toHaveBeenCalledWith("all");
  });

  it("only the selected tab is in the tab sequence", () => {
    render(<PeriodSelector value="30d" onChange={vi.fn()} ariaLabel="period" labels={labels} />);

    expect(screen.getByRole("tab", { name: "7d" })).toHaveAttribute("tabIndex", "-1");
    expect(screen.getByRole("tab", { name: "30d" })).toHaveAttribute("tabIndex", "0");
    expect(screen.getByRole("tab", { name: "All" })).toHaveAttribute("tabIndex", "-1");
  });
});
