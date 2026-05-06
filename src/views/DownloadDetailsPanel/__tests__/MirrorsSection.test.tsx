import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { MirrorsSection } from "../MirrorsSection";
import type { DownloadDetailView } from "@/types/download";

function makeDetail(overrides: Partial<DownloadDetailView> = {}): DownloadDetailView {
  return {
    id: "dl-1",
    fileName: "file.bin",
    url: "https://primary.example.com/file.bin",
    sourceHostname: "primary.example.com",
    state: "Downloading",
    progressPercent: 0,
    speedBytesPerSec: 0,
    downloadedBytes: 0,
    totalBytes: null,
    etaSeconds: null,
    segments: [],
    checksumExpected: null,
    checksumComputed: null,
    checksumAlgorithm: null,
    destinationPath: "/tmp/file.bin",
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 5,
    mirrors: [],
    currentMirrorIndex: 0,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

describe("MirrorsSection", () => {
  it("renders nothing when no mirrors are configured", () => {
    const { container } = render(<MirrorsSection download={makeDetail()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("highlights the active mirror and lists alternatives", () => {
    const detail = makeDetail({
      currentMirrorIndex: 1,
      mirrors: [
        { url: "https://m1.example.com/f", priority: 80, country: "US" },
        { url: "https://m2.example.com/f", priority: 50, country: null },
        { url: "https://m3.example.com/f", priority: 20, country: "DE" },
      ],
    });

    render(<MirrorsSection download={detail} />);

    // Active mirror is duplicated (header + list row) so use getAllByText.
    expect(screen.getAllByText("m2.example.com")).toHaveLength(2);
    expect(screen.getByText(/Priority 50/)).toBeTruthy();
    expect(screen.getByText(/Alternatives \(3\)/)).toBeTruthy();
    expect(screen.getByText("m1.example.com")).toBeTruthy();
    expect(screen.getByText("m3.example.com")).toBeTruthy();

    // The active mirror's list item carries aria-current.
    const activeItems = screen
      .getAllByRole("listitem")
      .filter((li) => li.getAttribute("aria-current") === "true");
    expect(activeItems).toHaveLength(1);
    expect(activeItems[0].textContent).toContain("m2.example.com");
  });

  it("shows priority and country tags per mirror", () => {
    const detail = makeDetail({
      mirrors: [{ url: "https://only.example.com/f", priority: 60, country: "FR" }],
    });
    render(<MirrorsSection download={detail} />);
    expect(screen.getByText(/P60 FR/)).toBeTruthy();
  });
});
