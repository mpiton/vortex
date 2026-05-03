import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StateIndicator } from "../StateIndicator";
import type { DownloadState } from "@/types/download";

const ALL_STATES: DownloadState[] = [
  "Queued",
  "Downloading",
  "Paused",
  "Waiting",
  "Retry",
  "Error",
  "Completed",
  "Checking",
  "Extracting",
];

describe("StateIndicator", () => {
  it.each(ALL_STATES)("should render state label for %s", (state) => {
    render(<StateIndicator state={state} />);
    expect(screen.getByText(state)).toBeInTheDocument();
  });

  it("should render a colored dot", () => {
    const { container } = render(<StateIndicator state="Downloading" />);
    const dot = container.querySelector(".rounded-full");
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveClass("bg-green-500");
  });

  it("should animate downloading state", () => {
    const { container } = render(<StateIndicator state="Downloading" />);
    const dot = container.querySelector(".rounded-full");
    expect(dot).toHaveClass("animate-pulse");
  });

  it("should show error state in red", () => {
    const { container } = render(<StateIndicator state="Error" />);
    const dot = container.querySelector(".rounded-full");
    expect(dot).toHaveClass("bg-red-500");
  });

  it("should show an error details button only for Error state with an error message", () => {
    render(<StateIndicator state="Error" errorMessage="certificate has expired" />);
    expect(screen.getByRole("button", { name: "Show download error" })).toBeInTheDocument();
  });

  it("should not show an error details button for Retry state", () => {
    render(<StateIndicator state="Retry" errorMessage="certificate has expired" />);
    expect(screen.queryByRole("button", { name: "Show download error" })).not.toBeInTheDocument();
  });

  it("should open a click popover with the raw backend error message", async () => {
    const user = userEvent.setup();
    render(<StateIndicator state="Error" errorMessage="certificate has expired" />);

    await user.click(screen.getByRole("button", { name: "Show download error" }));

    expect(screen.getByText("certificate has expired")).toBeInTheDocument();
  });
});
