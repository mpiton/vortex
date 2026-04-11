import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SizeEstimate } from "../MediaGrabberDialog/SizeEstimate";

describe("SizeEstimate", () => {
  it("should calculate correct size for 1080p 10-minute video", () => {
    render(<SizeEstimate quality="1080p" format="mp4" duration={600} />);
    // 5000 kbps * 1000 * 600 / 8 = 375_000_000 bytes ≈ 357.63 MB
    expect(screen.getByText(/Estimated Size: 357\.63 MB/)).toBeInTheDocument();
  });

  it("should calculate correct size for audio_only", () => {
    render(<SizeEstimate quality="audio_only" format="m4a" duration={600} />);
    // 192 kbps * 1000 * 600 / 8 = 14_400_000 bytes ≈ 13.73 MB
    expect(screen.getByText(/Estimated Size: 13\.73 MB/)).toBeInTheDocument();
  });

  it("should show quality and format info", () => {
    render(<SizeEstimate quality="720p" format="webm" duration={300} />);
    expect(screen.getByText(/720p WEBM/)).toBeInTheDocument();
    expect(screen.getByText(/5m video/)).toBeInTheDocument();
  });

  it("should use fallback bitrate for unknown quality", () => {
    render(<SizeEstimate quality="unknown" format="mp4" duration={60} />);
    // 2500 kbps fallback * 1000 * 60 / 8 = 18_750_000 bytes ≈ 17.88 MB
    expect(screen.getByText(/Estimated Size: 17\.88 MB/)).toBeInTheDocument();
  });

  it("should update when quality changes", () => {
    const { rerender } = render(
      <SizeEstimate quality="480p" format="mp4" duration={600} />,
    );
    expect(screen.getByText(/480p MP4/)).toBeInTheDocument();

    rerender(<SizeEstimate quality="4k" format="mp4" duration={600} />);
    expect(screen.getByText(/4k MP4/)).toBeInTheDocument();
  });
});
