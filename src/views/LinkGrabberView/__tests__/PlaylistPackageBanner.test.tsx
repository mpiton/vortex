import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import "@/i18n/i18n";
import { PlaylistPackageBanner } from "../MediaGrabberDialog/PlaylistPackageBanner";

describe("PlaylistPackageBanner", () => {
  it("should render the will-create message with package name and item count", () => {
    render(<PlaylistPackageBanner packageName="Holiday Mix" itemCount={7} />);
    const banner = screen.getByTestId("playlist-package-banner");
    expect(banner.textContent).toContain("Holiday Mix");
    expect(banner.textContent).toContain("7");
  });

  it("should render the will-reuse message when willReuseExisting is true", () => {
    render(
      <PlaylistPackageBanner
        packageName="Existing Pack"
        itemCount={3}
        willReuseExisting
      />,
    );
    const banner = screen.getByTestId("playlist-package-banner");
    expect(banner.textContent?.toLowerCase()).toMatch(/reuse|réutilisation/);
    expect(banner.textContent).toContain("Existing Pack");
  });

  it("should fall back to the default name when packageName is blank", () => {
    render(<PlaylistPackageBanner packageName="   " itemCount={5} />);
    const banner = screen.getByTestId("playlist-package-banner");
    expect(banner.textContent).toContain("Playlist");
  });

  it("should expose role=status for screen readers", () => {
    render(<PlaylistPackageBanner packageName="X" itemCount={1} />);
    expect(screen.getByRole("status")).toBeInTheDocument();
  });
});
