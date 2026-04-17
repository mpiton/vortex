import { beforeAll, beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { MediaGrabberDialog } from "../MediaGrabberDialog";
import type { ResolvedLink } from "../types";
import type { MediaMetadata } from "@/types/media";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

const mockInvoke = vi.mocked(invoke);

const mockMediaLink: ResolvedLink = {
  id: "media-1",
  originalUrl: "https://youtube.com/watch?v=test123",
  resolvedUrl: "https://youtube.com/watch?v=test123",
  filename: "Test Video.mp4",
  sizeBytes: null,
  status: "online",
  moduleName: "youtube",
  isMedia: true,
  mediaType: "video",
};

const mockMetadata: MediaMetadata = {
  title: "Test Video Title",
  thumbnailUrl: "https://img.youtube.com/test.jpg",
  durationSeconds: 600,
  isPlaylist: false,
  availableQualities: [
    { quality: "1080p", height: 1080, width: 1920, fps: 30, bitrateKbps: 5000 },
    { quality: "720p", height: 720, width: 1280, fps: 30, bitrateKbps: 2500 },
    { quality: "480p", height: 480, width: 854, fps: 30, bitrateKbps: 1000 },
  ],
  availableFormats: ["mp4", "webm"],
  availableAudioFormats: ["m4a", "mp3", "opus"],
  availableSubtitles: [
    { code: "en", name: "English" },
    { code: "fr", name: "Français" },
  ],
};

const mockPlaylistMetadata: MediaMetadata = {
  ...mockMetadata,
  isPlaylist: true,
  playlistItems: [
    { id: "v1", title: "Video 1", durationSeconds: 120 },
    { id: "v2", title: "Video 2", durationSeconds: 240 },
    { id: "v3", title: "Video 3", durationSeconds: 180 },
  ],
};

beforeAll(() => {
  Element.prototype.hasPointerCapture = vi.fn().mockReturnValue(false);
  Element.prototype.setPointerCapture = vi.fn();
  Element.prototype.releasePointerCapture = vi.fn();
  Element.prototype.scrollIntoView = vi.fn();

  global.ResizeObserver = class {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  } as unknown as typeof ResizeObserver;
});

function renderDialog(
  props: Partial<React.ComponentProps<typeof MediaGrabberDialog>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  const defaultProps = {
    link: mockMediaLink,
    open: true,
    onOpenChange: vi.fn(),
    onConfirm: vi.fn(),
    ...props,
  };

  return {
    ...render(
      <QueryClientProvider client={queryClient}>
        <MediaGrabberDialog {...defaultProps} />
      </QueryClientProvider>,
    ),
    onConfirm: defaultProps.onConfirm,
    onOpenChange: defaultProps.onOpenChange,
  };
}

describe("MediaGrabberDialog", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should show loading skeleton while fetching metadata", () => {
    mockInvoke.mockReturnValue(new Promise(() => {}));
    renderDialog();
    expect(screen.getByText("Media Grabber Options")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeDisabled();
  });

  it("should display metadata when loaded", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    expect(
      await screen.findByText("Test Video Title"),
    ).toBeInTheDocument();
  });

  it("should show no-metadata message when metadata is null", async () => {
    mockInvoke.mockResolvedValue(null);
    renderDialog();

    expect(
      await screen.findByText("No metadata available for this link"),
    ).toBeInTheDocument();
  });

  it("should show error message with retry when metadata fetch fails", async () => {
    mockInvoke.mockRejectedValue(new Error("Network error"));
    renderDialog();

    expect(
      await screen.findByText("Failed to load media metadata"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Retry" }),
    ).toBeInTheDocument();
  });

  it("should show quality selector with available qualities", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    expect(await screen.findByText("1080p")).toBeInTheDocument();
    expect(screen.getByText("720p")).toBeInTheDocument();
    expect(screen.getByText("480p")).toBeInTheDocument();
  });

  it("should show format selector buttons", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.getByRole("button", { name: /mp4/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /webm/i })).toBeInTheDocument();
  });

  it("should toggle audio only mode and show audio formats", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");

    const audioSwitch = screen.getByRole("switch");
    await user.click(audioSwitch);

    expect(screen.getByRole("button", { name: /m4a/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /mp3/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /opus/i })).toBeInTheDocument();
  });

  it("should hide video quality when audio only is enabled", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.getByText("Video Quality")).toBeInTheDocument();

    const audioSwitch = screen.getByRole("switch");
    await user.click(audioSwitch);

    expect(screen.queryByText("Video Quality")).not.toBeInTheDocument();
  });

  it("should show subtitle selector with available languages", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.getByText("Subtitles")).toBeInTheDocument();
    expect(screen.getByText("English (en)")).toBeInTheDocument();
    expect(screen.getByText("Français (fr)")).toBeInTheDocument();
  });

  it("should allow multi-select of subtitles", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");

    const enCheckbox = screen.getByRole("checkbox", {
      name: "English (en)",
    });
    const frCheckbox = screen.getByRole("checkbox", {
      name: "Français (fr)",
    });

    await user.click(enCheckbox);
    await user.click(frCheckbox);

    expect(enCheckbox).toBeChecked();
    expect(frCheckbox).toBeChecked();
  });

  it("should hide subtitle selector when no subtitles available", async () => {
    mockInvoke.mockResolvedValue({
      ...mockMetadata,
      availableSubtitles: [],
    });
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.queryByText("Subtitles")).not.toBeInTheDocument();
  });

  it("should show size estimate", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.getByText(/Estimated Size:/)).toBeInTheDocument();
  });

  it("should call onConfirm with correct options on Download click", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    const { onConfirm, onOpenChange } = renderDialog();

    await screen.findByText("Test Video Title");

    await user.click(screen.getByRole("button", { name: "Download" }));

    expect(onConfirm).toHaveBeenCalledWith({
      quality: "1080p",
      format: "mp4",
      subtitles: [],
      audioOnly: false,
      playlistItems: [],
      title: "Test Video Title",
    });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("should call onConfirm with audio_only quality when audio mode is on", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    const { onConfirm } = renderDialog();

    await screen.findByText("Test Video Title");

    await user.click(screen.getByRole("switch"));
    await user.click(screen.getByRole("button", { name: "Download" }));

    expect(onConfirm).toHaveBeenCalledWith(
      expect.objectContaining({
        quality: "audio_only",
        format: "m4a",
        audioOnly: true,
      }),
    );
  });

  it("should close dialog on Cancel click", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();

    await screen.findByText("Test Video Title");
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("MediaGrabberDialog - Playlist", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should show playlist section for playlist content", async () => {
    mockInvoke.mockResolvedValue(mockPlaylistMetadata);
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.getByText("Playlist (3 items)")).toBeInTheDocument();
    expect(screen.getByText("Video 1")).toBeInTheDocument();
    expect(screen.getByText("Video 2")).toBeInTheDocument();
    expect(screen.getByText("Video 3")).toBeInTheDocument();
  });

  it("should select all playlist items with Select All button", async () => {
    mockInvoke.mockResolvedValue(mockPlaylistMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");
    await user.click(screen.getByRole("button", { name: "Select All" }));

    const playlistSection = screen.getByText("Playlist (3 items)").closest("section");
    const checkboxes = within(playlistSection!).getAllByRole("checkbox");
    for (const checkbox of checkboxes) {
      expect(checkbox).toBeChecked();
    }
  });

  it("should toggle individual playlist items", async () => {
    mockInvoke.mockResolvedValue(mockPlaylistMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");

    const playlistSection = screen.getByText("Playlist (3 items)").closest("section");
    const checkboxes = within(playlistSection!).getAllByRole("checkbox");
    await user.click(checkboxes[0]);

    expect(checkboxes[0]).toBeChecked();
    expect(checkboxes[1]).not.toBeChecked();
  });

  it("should deselect all after selecting all", async () => {
    mockInvoke.mockResolvedValue(mockPlaylistMetadata);
    const user = userEvent.setup();
    renderDialog();

    await screen.findByText("Test Video Title");

    await user.click(screen.getByRole("button", { name: "Select All" }));
    await user.click(screen.getByRole("button", { name: "Deselect All" }));

    const playlistSection = screen.getByText("Playlist (3 items)").closest("section");
    const checkboxes = within(playlistSection!).getAllByRole("checkbox");
    for (const checkbox of checkboxes) {
      expect(checkbox).not.toBeChecked();
    }
  });

  it("should not show playlist section for non-playlist content", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    renderDialog();

    await screen.findByText("Test Video Title");
    expect(screen.queryByText(/Playlist/)).not.toBeInTheDocument();
  });
});

describe("MediaGrabberDialog - State Reset", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should reset selections when dialog reopens with new link", async () => {
    mockInvoke.mockResolvedValue(mockMetadata);
    const user = userEvent.setup();

    const altLink: ResolvedLink = {
      ...mockMediaLink,
      id: "media-2",
      originalUrl: "https://youtube.com/watch?v=alt456",
    };

    const altMetadata: MediaMetadata = {
      ...mockMetadata,
      title: "Alt Video",
      availableQualities: [
        { quality: "720p", height: 720, width: 1280, fps: 60, bitrateKbps: 3000 },
      ],
      availableFormats: ["webm"],
      availableAudioFormats: ["opus"],
    };

    const { onConfirm, rerender } = (() => {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false },
          mutations: { retry: false },
        },
      });
      const onConfirmFn = vi.fn();
      const result = render(
        <QueryClientProvider client={queryClient}>
          <MediaGrabberDialog
            link={mockMediaLink}
            open={true}
            onOpenChange={vi.fn()}
            onConfirm={onConfirmFn}
          />
        </QueryClientProvider>,
      );
      return { onConfirm: onConfirmFn, ...result };
    })();

    // Wait for first metadata
    await screen.findByText("Test Video Title");

    // Toggle audio only
    await user.click(screen.getByRole("switch"));

    // Now rerender with new link and new metadata
    mockInvoke.mockResolvedValue(altMetadata);

    rerender(
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: {
              queries: { retry: false },
              mutations: { retry: false },
            },
          })
        }
      >
        <MediaGrabberDialog
          link={altLink}
          open={true}
          onOpenChange={vi.fn()}
          onConfirm={onConfirm}
        />
      </QueryClientProvider>,
    );

    await screen.findByText("Alt Video");

    // Audio only should be reset
    await user.click(screen.getByRole("button", { name: "Download" }));

    expect(onConfirm).toHaveBeenCalledWith(
      expect.objectContaining({
        quality: "720p",
        format: "webm",
        audioOnly: false,
        subtitles: [],
        playlistItems: [],
      }),
    );
  });
});
