import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { invoke } from "@tauri-apps/api/core";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LinkGrabberView } from "../LinkGrabberView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

const mockInvoke = vi.mocked(invoke);

function renderWithProviders(initialEntry: string | { pathname: string; state?: unknown } = "/link-grabber") {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <TooltipProvider>
          <LinkGrabberView />
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("LinkGrabberView", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue([]);
  });

  it("should render the header title", () => {
    renderWithProviders();
    expect(screen.getByText("Link Grabber")).toBeInTheDocument();
  });

  it("should render the clipboard monitoring toggle", () => {
    renderWithProviders();
    expect(screen.getByText("Clipboard Monitoring")).toBeInTheDocument();
    expect(screen.getByRole("switch")).toBeInTheDocument();
  });

  it("should render PasteZone with Analyze Links button", () => {
    renderWithProviders();
    expect(
      screen.getByRole("button", { name: "Analyze Links" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("should not show filter/grouping/actions sections when no links resolved", () => {
    renderWithProviders();
    expect(
      screen.queryByRole("button", { name: "All" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Group Into Packages:"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Select All/ }),
    ).not.toBeInTheDocument();
  });

  it("should call link_resolve when Analyze Links is clicked", async () => {
    mockInvoke.mockResolvedValue([]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "link_resolve",
        expect.objectContaining({ urls: ["https://example.com/file.zip"] }),
      );
    });
  });

  it("should clear inline error when retry succeeds", async () => {
    mockInvoke
      .mockRejectedValueOnce(new Error("AppState not registered"))
      .mockResolvedValueOnce([
        {
          id: "1",
          originalUrl: "https://example.com/file.zip",
          resolvedUrl: "https://example.com/file.zip",
          filename: "file.zip",
          sizeBytes: 1024,
          status: "online",
          moduleName: "http",
          isMedia: false,
        },
      ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Failed to analyze links. Please try again.",
    );
    expect(screen.getByText("AppState not registered")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Select All (1)" }),
      ).toBeInTheDocument();
    });
  });

  it("should show filter/grouping/actions sections after links are resolved", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "1",
        originalUrl: "https://example.com/file.zip",
        resolvedUrl: "https://example.com/file.zip",
        filename: "file.zip",
        sizeBytes: 1024,
        status: "online",
        moduleName: "http",
        isMedia: false,
      },
    ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "All" })).toBeInTheDocument();
    });

    expect(screen.getByText("Group Into Packages:")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Select All (1)" }),
    ).toBeInTheDocument();
  });

  it("should clear resolved links when Clear is clicked", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "1",
        originalUrl: "https://example.com/file.zip",
        resolvedUrl: "https://example.com/file.zip",
        filename: "file.zip",
        sizeBytes: 1024,
        status: "online",
        moduleName: "http",
        isMedia: false,
      },
    ]);

    const user = userEvent.setup();
    renderWithProviders();

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip");
    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Select All (1)" })).toBeInTheDocument();
    });

    const clearButtons = screen.getAllByRole("button", { name: "Clear" });
    // ActionsBar "Clear" button is the destructive one (last among Clear buttons)
    await user.click(clearButtons[clearButtons.length - 1]);

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "All" }),
      ).not.toBeInTheDocument();
    });
  });

  it("should focus the paste textarea when opened from the global add-urls shortcut", async () => {
    renderWithProviders({
      pathname: "/link-grabber",
      state: { focusPaste: true },
    });

    await waitFor(() => {
      expect(screen.getByRole("textbox")).toHaveFocus();
    });
  });
});
