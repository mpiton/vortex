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
    set((state) => {
      // No-op when the incoming status is structurally identical: avoids
      // a needless object-ref change so subscribers (LinkRow rows,
      // ResolvedLinksSection) don't re-render on duplicate events.
      if (statusesEqual(state.statuses[url], status)) {
        return state;
      }
      return { statuses: { ...state.statuses, [url]: status } };
    }),
  setManyStatuses: (entries) =>
    set((state) => {
      let changed = false;
      const next = { ...state.statuses };
      for (const [url, status] of entries) {
        if (!statusesEqual(next[url], status)) {
          next[url] = status;
          changed = true;
        }
      }
      return changed ? { statuses: next } : state;
    }),
  reset: () =>
    set((state) =>
      Object.keys(state.statuses).length === 0 ? state : { statuses: {} },
    ),
}));

function statusesEqual(a: LinkProbeStatus | undefined, b: LinkProbeStatus): boolean {
  if (!a) return false;
  if (a.kind !== b.kind) return false;
  if (a.kind === "online" && b.kind === "online") {
    return (
      (a.filename ?? null) === (b.filename ?? null) &&
      (a.size ?? null) === (b.size ?? null) &&
      (a.resumable ?? null) === (b.resumable ?? null)
    );
  }
  return true;
}
