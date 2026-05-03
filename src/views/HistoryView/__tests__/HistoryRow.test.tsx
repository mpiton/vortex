import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { HistoryView } from "@/types/download";
import { HistoryRow } from "../HistoryRow";

function entry(overrides: Partial<HistoryView> = {}): HistoryView {
  return {
    entryId: "42",
    downloadId: "42",
    fileName: "movie.mkv",
    url: "https://videos.example.com/cdn/movie.mkv",
    totalBytes: 5 * 1024 * 1024,
    completedAt: 1700001000,
    durationSeconds: 125,
    avgSpeed: 200 * 1024,
    destinationPath: "/home/user/Downloads/movie.mkv",
    ...overrides,
  };
}

function renderRow(props?: Partial<HistoryView>) {
  const actions = {
    redownload: vi.fn(),
    copyUrl: vi.fn(),
    deleteEntry: vi.fn(),
    openFolder: vi.fn(),
  };
  render(
    <table>
      <tbody>
        <HistoryRow entry={entry(props)} actions={actions} />
      </tbody>
    </table>,
  );
  return { actions };
}

describe("HistoryRow", () => {
  it("should display file name, hostname, size, duration, and avg speed", () => {
    renderRow();
    expect(screen.getByText("movie.mkv")).toBeInTheDocument();
    expect(screen.getByText("videos.example.com")).toBeInTheDocument();
    expect(screen.getByText("5.00 MB")).toBeInTheDocument();
    expect(screen.getByText("2m 5s")).toBeInTheDocument();
    expect(screen.getByText("200.00 KB/s")).toBeInTheDocument();
  });

  it("should show a Completed badge by default", () => {
    renderRow();
    expect(screen.getByText("Completed")).toBeInTheDocument();
  });

  it("should call redownload when the re-download button is clicked", async () => {
    const { actions } = renderRow();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Re-download" }));
    expect(actions.redownload).toHaveBeenCalledTimes(1);
    expect(actions.redownload.mock.calls[0][0].entryId).toBe("42");
  });

  it("should expose data-entry-id on the row for querying", () => {
    renderRow();
    expect(screen.getByTestId("history-row")).toHaveAttribute("data-entry-id", "42");
  });
});
