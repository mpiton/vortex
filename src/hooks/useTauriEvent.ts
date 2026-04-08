import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';

export function useTauriEvent<T>(eventName: string, callback: (payload: T) => void): void {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen<T>(eventName, (event) => {
      if (!cancelled) {
        callbackRef.current(event.payload);
      }
    });
    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, [eventName]);
}
