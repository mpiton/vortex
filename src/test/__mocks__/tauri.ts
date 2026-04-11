import { vi } from "vitest";

// Mock @tauri-apps/api/core
export const invoke = vi.fn();

// Mock @tauri-apps/api/event
type ListenerCallback = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<ListenerCallback>>();

export const listen = vi.fn(
  async (event: string, handler: ListenerCallback) => {
    if (!listeners.has(event)) {
      listeners.set(event, new Set());
    }
    listeners.get(event)!.add(handler);
    return () => {
      listeners.get(event)?.delete(handler);
    };
  }
);

export function emitMockEvent(event: string, payload: unknown) {
  listeners.get(event)?.forEach((handler) => handler({ payload }));
}

export function clearMockListeners() {
  listeners.clear();
}
