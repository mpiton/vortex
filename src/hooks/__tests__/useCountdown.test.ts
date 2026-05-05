import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useCountdown } from "../useCountdown";

describe("useCountdown", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-05-05T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("should return remaining seconds and a formatted MM:SS label", () => {
    const until = Date.now() + 95_000;
    const { result } = renderHook(() => useCountdown(until));
    expect(result.current.remainingSeconds).toBe(95);
    expect(result.current.label).toBe("01:35");
    expect(result.current.expired).toBe(false);
  });

  it("should tick down once per second", () => {
    const until = Date.now() + 5_000;
    const { result } = renderHook(() => useCountdown(until));
    expect(result.current.remainingSeconds).toBe(5);

    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(result.current.remainingSeconds).toBe(4);

    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(result.current.remainingSeconds).toBe(2);
  });

  it("should clamp to zero and flag expired once the deadline passes", () => {
    const until = Date.now() + 1_000;
    const { result } = renderHook(() => useCountdown(until));
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(result.current.remainingSeconds).toBe(0);
    expect(result.current.label).toBe("00:00");
    expect(result.current.expired).toBe(true);
  });

  it("should treat null deadline as inactive (no countdown)", () => {
    const { result } = renderHook(() => useCountdown(null));
    expect(result.current.remainingSeconds).toBe(0);
    expect(result.current.label).toBe("00:00");
    expect(result.current.expired).toBe(true);
  });

  it("should handle hour-long waits with HH:MM:SS format", () => {
    const until = Date.now() + 3_725_000; // 1h 02m 05s
    const { result } = renderHook(() => useCountdown(until));
    expect(result.current.label).toBe("01:02:05");
  });

  it("should reset when the deadline prop changes", () => {
    const { result, rerender } = renderHook(({ until }: { until: number }) => useCountdown(until), {
      initialProps: { until: Date.now() + 5_000 },
    });
    expect(result.current.remainingSeconds).toBe(5);

    rerender({ until: Date.now() + 30_000 });
    expect(result.current.remainingSeconds).toBe(30);
  });
});
