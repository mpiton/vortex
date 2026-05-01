import { create } from "zustand";

/**
 * Backend-mirrored discriminated union for the per-URL probe outcome.
 * Matches the JSON shape emitted by the `link-status-updated` Tauri
 * event (see `tauri_bridge::link_status_payload`).
 */
export type LinkProbeStatus =
  | { kind: "checking" }
  | {
      kind: "online";
      filename?: string | null;
      size?: number | null;
      resumable?: boolean | null;
    }
  | { kind: "premiumOnly" }
  | { kind: "offline" }
  | { kind: "unknown" };

interface LinkGrabberState {
  /** Live status keyed by the URL the user pasted. */
  statuses: Record<string, LinkProbeStatus>;
  setStatus: (url: string, status: LinkProbeStatus) => void;
  setManyStatuses: (entries: Array<[string, LinkProbeStatus]>) => void;
  reset: () => void;
}

export const useLinkGrabberStore = create<LinkGrabberState>((set) => ({
  statuses: {},
  setStatus: (url, status) =>
    set((state) => ({
      statuses: { ...state.statuses, [url]: status },
    })),
  setManyStatuses: (entries) =>
    set((state) => {
      const next = { ...state.statuses };
      for (const [url, status] of entries) {
        next[url] = status;
      }
      return { statuses: next };
    }),
  reset: () => set({ statuses: {} }),
}));
