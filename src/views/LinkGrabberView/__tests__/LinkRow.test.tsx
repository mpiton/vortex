import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useLinkGrabberStore } from "@/stores/linkGrabberStore";
import { LinkRow } from "../LinkRow";
import type { DuplicateCheck, DuplicateSource, ResolvedLink } from "../types";

beforeEach(() => {
  useLinkGrabberStore.getState().reset();
});

const baseLink: ResolvedLink = {
  id: "1",
  originalUrl: "https://example.com/file.zip",
  resolvedUrl: "https://example.com/file.zip",
  filename: "file.zip",
  sizeBytes: 1024,
  status: "online",
  moduleName: "core-http",
  isMedia: false,
};

function makeDuplicate(
  source: DuplicateSource | null,
  isDuplicate = true,
  existingFilename: string | null = "file.zip",
): DuplicateCheck {
  return {
    url: baseLink.originalUrl,
    isDuplicate,
    source,
    existingId: source ? "42" : null,
    existingFilename,
  };
}

function renderRow(link: ResolvedLink) {
  return render(
    <TooltipProvider>
      <LinkRow link={link} selected={false} onSelect={vi.fn()} />
    </TooltipProvider>,
  );
}

describe("LinkRow duplicate badge", () => {
  it("does not render the duplicate badge for a unique link", () => {
    renderRow(baseLink);
    expect(screen.queryByText(/Already in/)).not.toBeInTheDocument();
  });

  it("renders an 'Already in active' badge when source is active", () => {
    renderRow({ ...baseLink, duplicate: makeDuplicate("active") });
    expect(screen.getByText("Already in active")).toBeInTheDocument();
  });

  it("renders an 'Already in history' badge when source is history", () => {
    renderRow({ ...baseLink, duplicate: makeDuplicate("history", true, "old.zip") });
    expect(screen.getByText("Already in history")).toBeInTheDocument();
  });

  it("does not render the badge when isDuplicate is false", () => {
    renderRow({ ...baseLink, duplicate: makeDuplicate(null, false, null) });
    expect(screen.queryByText(/Already in/)).not.toBeInTheDocument();
  });

  it("exposes data-duplicate=active on the row when active", () => {
    renderRow({ ...baseLink, duplicate: makeDuplicate("active") });
    const row = screen.getByTestId(`link-row-${baseLink.originalUrl}`);
    expect(row.dataset.duplicate).toBe("active");
  });

  it("exposes data-duplicate=none on a unique link", () => {
    renderRow(baseLink);
    const row = screen.getByTestId(`link-row-${baseLink.originalUrl}`);
    expect(row.dataset.duplicate).toBe("none");
  });
});
