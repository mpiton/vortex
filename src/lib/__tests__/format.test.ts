import { describe, it, expect } from "vitest";
import { formatEta, formatSpeed, formatBytes } from "@/lib/format";

describe("formatEta", () => {
  it("should return dash for null", () => {
    expect(formatEta(null)).toBe("—");
  });

  it("should return dash for zero", () => {
    expect(formatEta(0)).toBe("—");
  });

  it("should return dash for negative", () => {
    expect(formatEta(-10)).toBe("—");
  });

  it("should format seconds only", () => {
    expect(formatEta(45)).toBe("45s");
  });

  it("should format minutes and seconds", () => {
    expect(formatEta(125)).toBe("2m 5s");
  });

  it("should format hours and minutes", () => {
    expect(formatEta(3661)).toBe("1h 1m");
  });

  it("should format days and hours", () => {
    expect(formatEta(90000)).toBe("1d 1h");
  });
});

describe("formatSpeed", () => {
  it("should return 0 B/s for zero", () => {
    expect(formatSpeed(0)).toBe("0 B/s");
  });

  it("should return 0 B/s for negative", () => {
    expect(formatSpeed(-100)).toBe("0 B/s");
  });

  it("should format bytes per second", () => {
    expect(formatSpeed(512)).toBe("512 B/s");
  });

  it("should format kilobytes per second", () => {
    expect(formatSpeed(1024 * 50)).toBe("50.00 KB/s");
  });

  it("should format megabytes per second", () => {
    expect(formatSpeed(1024 * 1024 * 5.5)).toBe("5.50 MB/s");
  });

  it("should format gigabytes per second", () => {
    expect(formatSpeed(1024 * 1024 * 1024 * 1.2)).toBe("1.20 GB/s");
  });
});

describe("formatBytes", () => {
  it("should return 0 B for null", () => {
    expect(formatBytes(null)).toBe("0 B");
  });

  it("should return 0 B for zero", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("should format bytes", () => {
    expect(formatBytes(100)).toBe("100 B");
  });

  it("should format kilobytes", () => {
    expect(formatBytes(2048)).toBe("2.00 KB");
  });

  it("should format megabytes", () => {
    expect(formatBytes(1024 * 1024 * 3.7)).toBe("3.70 MB");
  });

  it("should format terabytes", () => {
    expect(formatBytes(1024 * 1024 * 1024 * 1024 * 2)).toBe("2.00 TB");
  });
});
