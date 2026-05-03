import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { SpeedSparkline } from "../SpeedSparkline";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

const mockUseSpeedHistory = vi.fn();
vi.mock("@/hooks/useSpeedHistory", () => ({
  useSpeedHistory: (id: string) => mockUseSpeedHistory(id),
}));

describe("SpeedSparkline", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should show no history message with less than 2 samples", () => {
    mockUseSpeedHistory.mockReturnValue([{ time: Date.now(), speed: 1024 }]);
    render(<SpeedSparkline downloadId="dl-1" />);
    expect(screen.getByText("No history yet")).toBeInTheDocument();
  });

  it("should render SVG with enough samples", () => {
    const now = Date.now();
    mockUseSpeedHistory.mockReturnValue([
      { time: now - 4000, speed: 512000 },
      { time: now - 2000, speed: 1024000 },
      { time: now, speed: 768000 },
    ]);
    const { container } = render(<SpeedSparkline downloadId="dl-1" />);
    expect(container.querySelector("svg")).toBeInTheDocument();
    expect(container.querySelector("polyline")).toBeInTheDocument();
  });
});
