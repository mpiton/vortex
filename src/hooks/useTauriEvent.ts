import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export function useTauriEvent<T>(eventName: string, callback: (payload: T) => void): void {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    let cancelled = false;
    let unlistenFn: (() => void) | undefined;

    listen<T>(eventName, (event) => {
      if (!cancelled) {
        callbackRef.current(event.payload);
      }
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlistenFn = fn;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlistenFn?.();
    };
  }, [eventName]);
}
