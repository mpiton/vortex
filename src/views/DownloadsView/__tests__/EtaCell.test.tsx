import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { useDownloadStore } from "@/stores/downloadStore";
import { EtaCell } from "../EtaCell";

beforeEach(() => {
  useDownloadStore.setState({ progressMap: {} });
});

describe("EtaCell", () => {
  it("should show dash when no progress data", () => {
    render(<EtaCell downloadId="1" />);
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("should show dash when speed is zero", () => {
    useDownloadStore.setState({
      progressMap: {
        "1": {
          id: "1",
          downloadedBytes: 5000,
          totalBytes: 10000,
          speedBytesPerSec: 0,
          lastSampleBytes: 5000,
          lastSampleTime: Date.now(),
        },
      },
    });
    render(<EtaCell downloadId="1" />);
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("should show formatted ETA when speed available", () => {
    useDownloadStore.setState({
      progressMap: {
        "1": {
          id: "1",
          downloadedBytes: 5000,
          totalBytes: 10000,
          speedBytesPerSec: 100,
          lastSampleBytes: 5000,
          lastSampleTime: Date.now(),
        },
      },
    });
    render(<EtaCell downloadId="1" />);
    // 5000 bytes remaining / 100 bytes per sec = 50 seconds
    expect(screen.getByText("50s")).toBeInTheDocument();
  });
});
