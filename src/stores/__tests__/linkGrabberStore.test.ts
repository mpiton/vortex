import { describe, it, expect, beforeEach } from "vitest";
import { useLinkGrabberStore } from "@/stores/linkGrabberStore";

beforeEach(() => {
  useLinkGrabberStore.getState().reset();
});

describe("useLinkGrabberStore", () => {
  it("starts with an empty status map", () => {
    expect(useLinkGrabberStore.getState().statuses).toEqual({});
  });

  it("records the latest status for a URL via setStatus", () => {
    useLinkGrabberStore.getState().setStatus("https://a/", { kind: "checking" });
    expect(useLinkGrabberStore.getState().statuses["https://a/"]).toEqual({
      kind: "checking",
    });

    useLinkGrabberStore.getState().setStatus("https://a/", {
      kind: "online",
      filename: "file.zip",
      size: 1024,
      resumable: true,
    });
    expect(useLinkGrabberStore.getState().statuses["https://a/"]).toEqual({
      kind: "online",
      filename: "file.zip",
      size: 1024,
      resumable: true,
    });
  });

  it("keeps the existing status of other URLs untouched on set", () => {
    useLinkGrabberStore.getState().setStatus("https://a/", { kind: "checking" });
    useLinkGrabberStore.getState().setStatus("https://b/", { kind: "offline" });
    expect(useLinkGrabberStore.getState().statuses).toEqual({
      "https://a/": { kind: "checking" },
      "https://b/": { kind: "offline" },
    });
  });

  it("resets the entire map on reset", () => {
    useLinkGrabberStore.getState().setStatus("https://a/", { kind: "offline" });
    useLinkGrabberStore.getState().reset();
    expect(useLinkGrabberStore.getState().statuses).toEqual({});
  });

  it("setManyStatuses applies every entry atomically", () => {
    useLinkGrabberStore.getState().setManyStatuses([
      ["https://a/", { kind: "checking" }],
      ["https://b/", { kind: "checking" }],
      ["https://c/", { kind: "unknown" }],
    ]);
    expect(useLinkGrabberStore.getState().statuses).toEqual({
      "https://a/": { kind: "checking" },
      "https://b/": { kind: "checking" },
      "https://c/": { kind: "unknown" },
    });
  });
});
