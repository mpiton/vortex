import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useLinkGrabberStore, type LinkProbeStatus } from "@/stores/linkGrabberStore";

interface LinkStatusEventPayload {
  url: string;
  status: LinkProbeStatus;
}

/**
 * Subscribe to the backend `link-status-updated` Tauri event and
 * forward each payload into the Link Grabber Zustand store. Mounting
 * this hook on the Link Grabber view is enough — there is no manual
 * unsubscribe to perform: `useTauriEvent` cleans up on unmount.
 */
export function useLinkStatusEvents(): void {
  const setStatus = useLinkGrabberStore((s) => s.setStatus);
  useTauriEvent<LinkStatusEventPayload>("link-status-updated", (payload) => {
    setStatus(payload.url, payload.status);
  });
}
