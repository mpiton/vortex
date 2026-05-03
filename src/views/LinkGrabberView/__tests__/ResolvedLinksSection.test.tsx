import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useLinkGrabberStore } from "@/stores/linkGrabberStore";
import { ResolvedLinksSection, applyFilter, groupLinks } from "../ResolvedLinksSection";
import type { ResolvedLink } from "../types";

beforeEach(() => {
  useLinkGrabberStore.getState().reset();
});

const MOCK_LINKS: ResolvedLink[] = [
  {
    id: "1",
    originalUrl: "https://example.com/file.zip",
    resolvedUrl: "https://example.com/file.zip",
    filename: "file.zip",
    sizeBytes: 1048576,
    status: "online",
    moduleName: "core-http",
    isMedia: false,
  },
  {
    id: "2",
    originalUrl: "https://youtube.com/watch?v=abc",
    resolvedUrl: null,
    filename: null,
    sizeBytes: null,
    status: "online",
    moduleName: "youtube",
    isMedia: true,
    mediaType: "video",
  },
  {
    id: "3",
    originalUrl: "https://example.com/dead-link",
    resolvedUrl: null,
    filename: null,
    sizeBytes: null,
    status: "offline",
    moduleName: "core-http",
    isMedia: false,
  },
];

function renderWithProvider(ui: React.ReactElement) {
  return render(<TooltipProvider>{ui}</TooltipProvider>);
}

describe("groupLinks", () => {
  it("groups by hostname", () => {
    const result = groupLinks(MOCK_LINKS, "hostname");
    expect(Object.keys(result)).toContain("example.com");
    expect(Object.keys(result)).toContain("youtube.com");
  });

  it("groups by extension", () => {
    const result = groupLinks(MOCK_LINKS, "extension");
    expect(Object.keys(result)).toContain("ZIP");
  });

  it("groups by type", () => {
    const result = groupLinks(MOCK_LINKS, "type");
    expect(Object.keys(result)).toContain("Media");
    expect(Object.keys(result)).toContain("Other");
  });
});

describe("ResolvedLinksSection", () => {
  it("filters links by status online", () => {
    renderWithProvider(
      <ResolvedLinksSection
        links={MOCK_LINKS}
        filter="online"
        groupingMode="none"
        selectedIds={[]}
        onSelectIds={vi.fn()}
      />,
    );
    expect(screen.getByText("file.zip")).toBeInTheDocument();
    expect(screen.queryByText("https://example.com/dead-link")).not.toBeInTheDocument();
  });

  it("filters links by media", () => {
    renderWithProvider(
      <ResolvedLinksSection
        links={MOCK_LINKS}
        filter="media"
        groupingMode="none"
        selectedIds={[]}
        onSelectIds={vi.fn()}
      />,
    );
    // Only the media link (id=2) should be visible — its filename is null so originalUrl is shown
    expect(screen.getByText("https://youtube.com/watch?v=abc")).toBeInTheDocument();
    expect(screen.queryByText("file.zip")).not.toBeInTheDocument();
  });

  it("group checkbox selects all links in group", async () => {
    const user = userEvent.setup();
    const onSelectIds = vi.fn();
    renderWithProvider(
      <ResolvedLinksSection
        links={MOCK_LINKS}
        filter="all"
        groupingMode="hostname"
        selectedIds={[]}
        onSelectIds={onSelectIds}
      />,
    );
    // Click the checkbox for the example.com group (links id=1 and id=3)
    const groupCheckboxes = screen.getAllByRole("checkbox", {
      name: /Select all in example\.com/i,
    });
    await user.click(groupCheckboxes[0]);
    expect(onSelectIds).toHaveBeenCalledWith(expect.arrayContaining(["1", "3"]));
  });

  it("individual checkbox toggles selection", async () => {
    const user = userEvent.setup();
    const onSelectIds = vi.fn();
    renderWithProvider(
      <ResolvedLinksSection
        links={MOCK_LINKS}
        filter="all"
        groupingMode="none"
        selectedIds={[]}
        onSelectIds={onSelectIds}
      />,
    );
    const linkCheckboxes = screen.getAllByRole("checkbox", {
      name: /Select link/i,
    });
    await user.click(linkCheckboxes[0]);
    expect(onSelectIds).toHaveBeenCalledWith(["1"]);
  });

  it("renders a retry button only when the live status is unknown", async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    const links: ResolvedLink[] = [
      {
        id: "u",
        originalUrl: "https://timeout/",
        resolvedUrl: null,
        filename: null,
        sizeBytes: null,
        status: "online",
        moduleName: "core-http",
        isMedia: false,
      },
      {
        id: "ok",
        originalUrl: "https://ok/",
        resolvedUrl: null,
        filename: "ok.zip",
        sizeBytes: null,
        status: "online",
        moduleName: "core-http",
        isMedia: false,
      },
    ];
    useLinkGrabberStore.getState().setStatus("https://timeout/", { kind: "unknown" });
    renderWithProvider(
      <ResolvedLinksSection
        links={links}
        filter="all"
        groupingMode="none"
        selectedIds={[]}
        onSelectIds={vi.fn()}
        onRetry={onRetry}
      />,
    );
    const retryButtons = screen.getAllByRole("button", { name: /retry-link-check/i });
    expect(retryButtons).toHaveLength(1);
    await user.click(retryButtons[0]);
    expect(onRetry).toHaveBeenCalledWith("https://timeout/");
  });

  it("Online filter prefers the live status from the store over the static one", () => {
    const links: ResolvedLink[] = [
      {
        id: "x",
        originalUrl: "https://maybe-online/",
        resolvedUrl: null,
        filename: null,
        sizeBytes: null,
        // Static state says "checking" but the live event has flipped to "offline".
        status: "checking",
        moduleName: "core-http",
        isMedia: false,
      },
    ];
    useLinkGrabberStore.getState().setStatus("https://maybe-online/", {
      kind: "offline",
    });
    renderWithProvider(
      <ResolvedLinksSection
        links={links}
        filter="online"
        groupingMode="none"
        selectedIds={[]}
        onSelectIds={vi.fn()}
      />,
    );
    expect(screen.queryByText("https://maybe-online/")).not.toBeInTheDocument();
  });
});

describe("applyFilter", () => {
  const link = (id: string, status: ResolvedLink["status"]): ResolvedLink => ({
    id,
    originalUrl: `https://${id}/`,
    resolvedUrl: null,
    filename: null,
    sizeBytes: null,
    status,
    moduleName: "core-http",
    isMedia: false,
  });

  it("filters online based on live status when present", () => {
    const links = [link("a", "checking"), link("b", "checking")];
    const result = applyFilter(links, "online", {
      "https://a/": { kind: "online", filename: null, size: null, resumable: null },
    });
    expect(result.map((l) => l.id)).toEqual(["a"]);
  });

  it("groups offline + unknown together for the offline filter", () => {
    const links = [link("a", "online"), link("b", "offline"), link("c", "checking")];
    const result = applyFilter(links, "offline", {
      "https://c/": { kind: "unknown" },
    });
    expect(result.map((l) => l.id).sort()).toEqual(["b", "c"]);
  });
});
