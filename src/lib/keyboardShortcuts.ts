export const SHORTCUT_ACTION_EVENT = 'vortex:shortcut-action';

export const SHORTCUT_ACTIONS = {
  downloadsFocusSearch: 'downloads.focus-search',
  downloadsSelectAll: 'downloads.select-all',
  downloadsToggleSelected: 'downloads.toggle-selected',
  downloadsRemoveSelected: 'downloads.remove-selected',
} as const;

export type ShortcutAction =
  (typeof SHORTCUT_ACTIONS)[keyof typeof SHORTCUT_ACTIONS];

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
    handler((event as CustomEvent<ShortcutAction>).detail);
  };

  window.addEventListener(SHORTCUT_ACTION_EVENT, listener as EventListener);
  return () => {
    window.removeEventListener(
      SHORTCUT_ACTION_EVENT,
      listener as EventListener,
    );
  };
}
