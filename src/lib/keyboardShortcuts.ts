export const SHORTCUT_ACTION_EVENT = 'vortex:shortcut-action';

export const SHORTCUT_ACTIONS = {
  downloadsFocusSearch: 'downloads.focus-search',
  downloadsSelectAll: 'downloads.select-all',
  downloadsToggleSelected: 'downloads.toggle-selected',
  downloadsRemoveSelected: 'downloads.remove-selected',
} as const;

export type ShortcutAction =
  (typeof SHORTCUT_ACTIONS)[keyof typeof SHORTCUT_ACTIONS];

function isShortcutAction(value: unknown): value is ShortcutAction {
  return (
    typeof value === 'string' &&
    Object.values(SHORTCUT_ACTIONS).includes(value as ShortcutAction)
  );
}

export function dispatchShortcutAction(action: ShortcutAction) {
  window.dispatchEvent(
    new CustomEvent<ShortcutAction>(SHORTCUT_ACTION_EVENT, {
      detail: action,
    }),
  );
}

export function subscribeShortcutAction(
  handler: (action: ShortcutAction) => void,
) {
  const listener = (event: Event) => {
    if (!(event instanceof CustomEvent)) return;

    const detail = (event as CustomEvent<unknown>).detail;
    if (isShortcutAction(detail)) {
      handler(detail);
    }
  };

  window.addEventListener(SHORTCUT_ACTION_EVENT, listener as EventListener);
  return () => {
    window.removeEventListener(
      SHORTCUT_ACTION_EVENT,
      listener as EventListener,
    );
  };
}
