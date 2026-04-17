# Globalize sonner feedback (issue #74) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Globalize `sonner` as the single UX feedback channel by adding a default `onError` toast in `useTauriMutation`, with explicit opt-out, and migrate every IPC mutation site to rely on it (success labels stay per-site via i18n).

**Architecture:** Extend `useTauriMutation` in `src/api/hooks.ts` with `silentError?: boolean` and `errorMessage?: (err) => string` options. When `onError` is not provided and `silentError !== true`, the hook automatically calls `toast.error(options?.errorMessage?.(err) ?? err.message)`. Everywhere a custom `onError` previously used local state for error display (e.g. `setResolveError`), we remove it and rely on the hook's default. Success messages remain explicit per-site because labels are business-specific.

**Tech Stack:** React 19, TanStack Query v5, `sonner` (via `src/lib/toast.ts` wrapper), react-i18next, Vitest.

**Dependency order:**
- Tasks 1 + 2 must land first (infrastructure).
- Tasks 3–8 touch disjoint files and can be parallelized (subagents).
- Task 9 closes the loop: verification, changelog, commit.

---

## Task 1: Extend `useTauriMutation` with default error toast

**Files:**
- Modify: `src/api/hooks.ts`
- Test: `src/api/__tests__/hooks.test.ts`

- [ ] **Step 1: Update the test file to cover the new default behavior**

Replace the `describe('useTauriMutation', ...)` block (lines 68–98) with this content — the existing two tests stay, three new ones are appended:

```ts
describe('useTauriMutation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should call tauriInvoke with command on mutate', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(tauriInvoke).toHaveBeenCalledWith('download_pause', { id: '1' });
  });

  it('should expose error when mutation fails', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('pause failed'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toBe('pause failed');
  });

  it('should surface toast.error by default when mutation fails and no onError is provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('boom'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).toHaveBeenCalledWith('boom');
  });

  it('should NOT surface toast.error when silentError is true', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('quiet'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause', { silentError: true }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('should NOT surface toast.error when a custom onError is provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('custom'));
    const customOnError = vi.fn();
    const { result } = renderHook(
      () => useTauriMutation('download_pause', { onError: customOnError }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(customOnError).toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('should use errorMessage mapper when provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('raw'));
    const { result } = renderHook(
      () =>
        useTauriMutation('download_pause', {
          errorMessage: (err) => `Mapped: ${err.message}`,
        }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).toHaveBeenCalledWith('Mapped: raw');
  });
});
```

Add at the top of the file (under the existing `vi.mock('@/api/client', ...)`):

```ts
vi.mock('@/lib/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));
```

And import right after the existing imports:

```ts
import { toast } from '@/lib/toast';
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/api/__tests__/hooks.test.ts`
Expected: 3 new tests FAIL (`silentError`, `errorMessage`, default toast). The existing 2 still pass.

- [ ] **Step 3: Update `src/api/hooks.ts` to implement default toast behavior**

Replace the full content of `src/api/hooks.ts` with:

```ts
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import type { UseQueryOptions } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { toast } from '@/lib/toast';

export function useTauriQuery<T>(
  command: string,
  args?: Record<string, unknown>,
  options?: Omit<UseQueryOptions<T, Error>, 'queryFn'>
) {
  return useQuery<T, Error>({
    queryKey: args ? [command, args] : [command],
    queryFn: () => tauriInvoke<T>(command, args),
    ...options,
  });
}

/**
 * Mutation hook for Tauri IPC commands.
 *
 * Error feedback contract:
 * - If `onError` is NOT provided AND `silentError !== true`, the hook
 *   automatically surfaces `toast.error(errorMessage?.(err) ?? err.message)`.
 * - If `onError` IS provided, the default toast is suppressed — you own
 *   the error UX (inline alert, navigate, custom toast, etc.).
 * - If `silentError === true`, no default toast and no custom onError
 *   required (use for background polling or retries handled upstream).
 *
 * Success feedback is always the caller's responsibility: pass an
 * `onSuccess` that calls `toast.success(t('<ns>.toast.<action>'))` with a
 * business-specific i18n label.
 */
interface UseTauriMutationOptions<TData, TVariables> {
  invalidateKeys?: readonly (readonly unknown[])[];
  onSuccess?: (data: TData, variables: TVariables, context: unknown) => void;
  onError?: (error: Error, variables: TVariables, context: unknown) => void;
  silentError?: boolean;
  errorMessage?: (err: Error) => string;
}

export function useTauriMutation<
  TData = unknown,
  TVariables extends Record<string, unknown> | void = Record<string, unknown>,
>(command: string, options?: UseTauriMutationOptions<TData, TVariables>) {
  const queryClientInstance = useQueryClient();

  return useMutation<TData, Error, TVariables>({
    mutationFn: (variables) =>
      tauriInvoke<TData>(command, variables as Record<string, unknown> | undefined),
    onSuccess: (data, variables, context) => {
      if (options?.invalidateKeys) {
        for (const key of options.invalidateKeys) {
          queryClientInstance.invalidateQueries({ queryKey: key });
        }
      }
      options?.onSuccess?.(data, variables, context);
    },
    onError: (error, variables, context) => {
      if (options?.onError) {
        options.onError(error, variables, context);
        return;
      }
      if (options?.silentError) return;
      toast.error(options?.errorMessage?.(error) ?? error.message);
    },
  });
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/api/__tests__/hooks.test.ts`
Expected: All 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/hooks.ts src/api/__tests__/hooks.test.ts
git commit -m "feat(api): add default error toast to useTauriMutation (#74)"
```

---

## Task 2: Mock `sonner` globally in test-setup

**Files:**
- Modify: `src/test-setup.ts`

- [ ] **Step 1: Append sonner mock at the end of `src/test-setup.ts`, BEFORE the final `beforeEach` block**

Insert after line 93 (the closing `});` of `matchMedia`) and BEFORE line 95 (the final `beforeEach`):

```ts
// Mock sonner globally so tests don't render actual toasts and can assert
// toast.error / toast.success calls when needed.
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    promise: vi.fn(),
  },
  Toaster: () => null,
}));
```

Also update the final `beforeEach` to clear sonner mocks:

```ts
beforeEach(() => {
  window.localStorage.removeItem("i18nextLng");
  vi.clearAllMocks();
});
```

- [ ] **Step 2: Run the full frontend suite**

Run: `npx vitest run`
Expected: All tests PASS. If any test was asserting on `toast` being rendered in the DOM, update it to import `toast` from `@/lib/toast` and assert on the mock calls.

- [ ] **Step 3: Commit**

```bash
git add src/test-setup.ts
git commit -m "test(ui): mock sonner globally in vitest setup (#74)"
```

---

## Task 3: Add i18n toast keys for plugins, settings, linkGrabber

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/fr.json`

- [ ] **Step 1: Locate the `plugins` namespace in `en.json` and add a `toast` subsection**

Inside the `plugins` object, append:

```json
    "toast": {
      "installSuccess": "Plugin \"{{name}}\" installed",
      "updateSuccess": "Plugin \"{{name}}\" updated",
      "refreshSuccess": "Plugin list refreshed",
      "disableSuccess": "Plugin disabled",
      "uninstallSuccess": "Plugin uninstalled"
    }
```

- [ ] **Step 2: Mirror in `fr.json`**

```json
    "toast": {
      "installSuccess": "Plugin « {{name}} » installé",
      "updateSuccess": "Plugin « {{name}} » mis à jour",
      "refreshSuccess": "Liste des plugins rafraîchie",
      "disableSuccess": "Plugin désactivé",
      "uninstallSuccess": "Plugin désinstallé"
    }
```

- [ ] **Step 3: Add a `settings.toast` subsection in `en.json`**

Inside the `settings` object:

```json
    "toast": {
      "updateSuccess": "Settings saved"
    }
```

- [ ] **Step 4: Mirror in `fr.json`**

```json
    "toast": {
      "updateSuccess": "Paramètres enregistrés"
    }
```

- [ ] **Step 5: Add a `linkGrabber.toast` subsection in `en.json`**

Inside the `linkGrabber` object:

```json
    "toast": {
      "resolveSuccess_one": "{{count}} link resolved",
      "resolveSuccess_other": "{{count}} links resolved",
      "downloadStarted": "Download started"
    }
```

- [ ] **Step 6: Mirror in `fr.json`**

```json
    "toast": {
      "resolveSuccess_one": "{{count}} lien analysé",
      "resolveSuccess_other": "{{count}} liens analysés",
      "downloadStarted": "Téléchargement démarré"
    }
```

- [ ] **Step 7: Add a `clipboard.toast` subsection in `en.json`**

If no `clipboard` namespace exists at the root, add it. Otherwise insert `toast` under it:

```json
  "clipboard": {
    "toast": {
      "enabled": "Clipboard monitoring enabled",
      "disabled": "Clipboard monitoring disabled"
    }
  }
```

- [ ] **Step 8: Mirror in `fr.json`**

```json
  "clipboard": {
    "toast": {
      "enabled": "Surveillance du presse-papiers activée",
      "disabled": "Surveillance du presse-papiers désactivée"
    }
  }
```

- [ ] **Step 9: Verify both JSON files parse**

Run: `node -e "JSON.parse(require('fs').readFileSync('src/i18n/locales/en.json','utf8')); JSON.parse(require('fs').readFileSync('src/i18n/locales/fr.json','utf8')); console.log('ok')"`
Expected: `ok`

- [ ] **Step 10: Commit**

```bash
git add src/i18n/locales/en.json src/i18n/locales/fr.json
git commit -m "i18n(ui): add toast keys for plugins/settings/linkGrabber/clipboard (#74)"
```

---

## Task 4: Migrate DownloadsView cluster (ActionsBar already canonical — add defaults to remaining mutations)

**Files:**
- Modify: `src/views/DownloadsView/ActionsBar.tsx` (lines 26–36)
- Modify: `src/views/DownloadsView/DownloadsTable.tsx` (around line 339–343)
- Modify: `src/views/DownloadsView/DownloadsView.tsx` (around line 31–40)

The goal is simple: `ActionsBar`'s `clearCompleted`/`clearFailed` are already canonical. The three leading mutations (`download_pause_all`, `download_resume_all`, `download_cancel`) and all the `DownloadsTable`/`DownloadsView` mutations have no `onError`. Thanks to Task 1 they now get a default toast automatically — **no code change is needed**, but we still want to (a) confirm the file compiles and tests pass, and (b) in `DownloadsTable.tsx` and `DownloadsView.tsx`, remove any dead `// TODO: error handling` comments if present.

- [ ] **Step 1: Read the three files and confirm no changes are required**

Read `src/views/DownloadsView/ActionsBar.tsx`, `DownloadsTable.tsx`, `DownloadsView.tsx`. Verify the mutations without `onError`:
- `download_pause_all`, `download_resume_all`, `download_cancel` (ActionsBar)
- `download_pause`, `download_resume`, `download_retry`, `download_remove`, `download_set_priority` (DownloadsTable)
- `download_pause`, `download_resume`, `download_remove` (DownloadsView)

They will now surface `toast.error(err.message)` by default — desired.

- [ ] **Step 2: Remove any `// TODO: error handling` / similar comments tied to these mutations**

Use Grep in these three files for `TODO.*error` or `FIXME.*error`. Remove obsolete comments only.

- [ ] **Step 3: Run the downloads view tests**

Run: `npx vitest run src/views/DownloadsView`
Expected: PASS.

- [ ] **Step 4: Commit (skip if no file changed)**

```bash
git add src/views/DownloadsView
git commit -m "refactor(ui): rely on default error toast for downloads mutations (#74)" || echo "no changes"
```

---

## Task 5: Migrate SettingsView sections

**Files:**
- Modify: `src/views/SettingsView/AppearanceSection.tsx`
- Modify: `src/views/SettingsView/GeneralSection.tsx`
- Modify: `src/views/SettingsView/DownloadsSection.tsx`
- Modify: `src/views/SettingsView/NetworkSection.tsx`
- Modify: `src/views/SettingsView/BrowserSection.tsx`
- Modify: `src/views/SettingsView/RemoteAccessSection.tsx`

All six sections call `useTauriMutation('settings_update', { invalidateKeys: [['settings_get']] })`. The `settings_update` mutation should:
- On success: call `toast.success(t('settings.toast.updateSuccess'))`.
- On failure: rely on the default toast (no `onError` override).

`AppearanceSection.tsx` has a custom `onSuccess` (`setTheme`). Keep it, append the toast call.

- [ ] **Step 1: In each section, import toast and useTranslation (if not already)**

At top of each file, ensure presence of:

```ts
import { useTranslation } from 'react-i18next';
import { toast } from '@/lib/toast';
```

And in each component body:

```ts
const { t } = useTranslation();
```

- [ ] **Step 2: Add `onSuccess` to `settings_update` in each section**

Example for `GeneralSection.tsx` (adapt to the variable names in each file):

```ts
const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>(
  'settings_update',
  {
    invalidateKeys: [['settings_get']],
    onSuccess: () => {
      toast.success(t('settings.toast.updateSuccess'));
    },
  },
);
```

For `AppearanceSection.tsx`, merge with existing `onSuccess`:

```ts
const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>(
  'settings_update',
  {
    invalidateKeys: [['settings_get']],
    onSuccess: (_data, variables) => {
      const nextTheme = variables.patch.theme;
      if (nextTheme === 'light' || nextTheme === 'dark' || nextTheme === 'auto') {
        setTheme(nextTheme);
      }
      toast.success(t('settings.toast.updateSuccess'));
    },
  },
);
```

Do NOT add `onError` — let the default toast surface the raw error.

- [ ] **Step 3: Run the settings tests**

Run: `npx vitest run src/views/SettingsView`
Expected: PASS. If an existing test asserts on `mutate` being called without checking toast, the test still passes — our toast mock captures calls silently.

- [ ] **Step 4: Commit**

```bash
git add src/views/SettingsView
git commit -m "feat(ui): add success toasts to settings sections (#74)"
```

---

## Task 6: Migrate PluginsView + usePluginStore (convert to `useTauriMutation`)

**Files:**
- Modify: `src/views/PluginsView.tsx` (around lines 45–53)
- Modify: `src/views/PluginsView/usePluginStore.ts`

`usePluginStore.ts` currently uses `useMutation` + `invoke` directly, bypassing our hook entirely. Migrate to `useTauriMutation` so it gains the default toast.

- [ ] **Step 1: Rewrite `usePluginStore.ts` to use `useTauriMutation`**

Replace the full file content with:

```ts
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import type { PluginStoreEntry } from "@/types/plugin-store";

const STORE_QUERY_KEY = ["plugin_store_list"] as const;

export function usePluginStore() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [installingNames, setInstallingNames] = useState<Set<string>>(new Set());
  const [updatingNames, setUpdatingNames] = useState<Set<string>>(new Set());

  const { data: entries = [], isLoading, isError } = useQuery({
    queryKey: STORE_QUERY_KEY,
    queryFn: () => invoke<PluginStoreEntry[]>("plugin_store_list"),
  });

  const refreshMutation = useTauriMutation<void, void>("plugin_store_refresh", {
    invalidateKeys: [STORE_QUERY_KEY],
    onSuccess: () => toast.success(t("plugins.toast.refreshSuccess")),
  });

  const installMutation = useTauriMutation<void, { name: string }>(
    "plugin_store_install",
    {
      invalidateKeys: [STORE_QUERY_KEY],
      onSuccess: (_data, variables) => {
        toast.success(t("plugins.toast.installSuccess", { name: variables.name }));
      },
    },
  );

  const updateMutation = useTauriMutation<void, { name: string }>(
    "plugin_store_update",
    {
      invalidateKeys: [STORE_QUERY_KEY],
      onSuccess: (_data, variables) => {
        toast.success(t("plugins.toast.updateSuccess", { name: variables.name }));
      },
    },
  );

  const trackInstall = (name: string) => {
    setInstallingNames((s) => new Set(s).add(name));
    installMutation.mutate(
      { name },
      {
        onSettled: () =>
          setInstallingNames((s) => {
            const next = new Set(s);
            next.delete(name);
            return next;
          }),
      },
    );
  };

  const trackUpdate = (name: string) => {
    setUpdatingNames((s) => new Set(s).add(name));
    updateMutation.mutate(
      { name },
      {
        onSettled: () =>
          setUpdatingNames((s) => {
            const next = new Set(s);
            next.delete(name);
            return next;
          }),
      },
    );
  };

  return {
    entries,
    isLoading,
    isError,
    refreshStore: () => refreshMutation.mutate(),
    installPlugin: trackInstall,
    updatePlugin: trackUpdate,
    isInstalling: (name: string) => installingNames.has(name),
    isUpdating: (name: string) => updatingNames.has(name),
    isRefreshing: refreshMutation.isPending,
  };
}
```

Note: `queryClient` is now unused in this file (invalidation is handled by `invalidateKeys`). If ESLint flags unused imports, remove `useQueryClient` import too. Keep `useQuery` / `invoke` for the list query.

Also remove the unused `queryClient` declaration if ESLint complains.

- [ ] **Step 2: Add toasts + default error in `PluginsView.tsx`**

For the two mutations at lines 45–53:

```ts
const { mutate: disablePlugin } = useTauriMutation<void, { id: string }>(
  "plugin_disable",
  {
    invalidateKeys: [["plugin_list"]],
    onSuccess: () => toast.success(t("plugins.toast.disableSuccess")),
  },
);

const { mutate: uninstallPlugin } = useTauriMutation<void, { id: string }>(
  "plugin_uninstall",
  {
    invalidateKeys: [["plugin_list"]],
    onSuccess: () => toast.success(t("plugins.toast.uninstallSuccess")),
  },
);
```

Ensure `import { toast } from '@/lib/toast';` and `const { t } = useTranslation();` are present. Adapt the TVariables types to match the existing signatures in the file.

- [ ] **Step 3: Run plugin tests**

Run: `npx vitest run src/views/PluginsView`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/views/PluginsView.tsx src/views/PluginsView/usePluginStore.ts
git commit -m "refactor(plugin): migrate plugin mutations to global toast feedback (#74)"
```

---

## Task 7: Migrate `LinkGrabberView` — drop inline `setResolveError`

**Files:**
- Modify: `src/views/LinkGrabberView/LinkGrabberView.tsx`
- Modify: `src/views/LinkGrabberView/PasteZone.tsx` (remove `errorMessage` prop wiring)

Decision: since the link-resolve error is an IPC failure (not form validation), the global toast is a better fit. Remove the local error state entirely. `PasteZone` keeps its prop optional for future form-validation errors but we stop threading `resolveError` through it.

- [ ] **Step 1: Update `LinkGrabberView.tsx`**

Replace lines 22 (state declaration) and the onSuccess/onError bodies:

- Remove: `const [resolveError, setResolveError] = useState<string | null>(null);`
- Remove: `setResolveError(null);` calls (lines 39, 66) and `setResolveError(error.message);` calls (lines 44, 60).
- Remove the prop `errorMessage={resolveError}` from the `<PasteZone>` JSX (line ~153).
- In the `link_resolve` mutation, keep `onSuccess` for state updates but let default error toast handle failures:

```ts
const { mutate: resolveLinks, isPending: isResolving } = useTauriMutation<
  ResolvedLink[],
  { urls: string[] }
>("link_resolve", {
  onSuccess: (resolved) => {
    setResolvedLinks(resolved);
    setSelectedLinkIds([]);
    toast.success(t("linkGrabber.toast.resolveSuccess", { count: resolved.length }));
  },
});
```

- In the `download_media_start` mutation, keep the navigate onSuccess and let default error toast handle failures:

```ts
const { mutate: startMediaDownload } = useTauriMutation<
  unknown,
  { url: string; quality: string; format: string; audioOnly: boolean; title?: string }
>("download_media_start", {
  onSuccess: () => {
    toast.success(t("linkGrabber.toast.downloadStarted"));
    void navigate("/");
  },
});
```

- Add import `import { toast } from '@/lib/toast';` at top of file.

- [ ] **Step 2: Update `PasteZone.tsx`**

Remove the `errorMessage` prop from the interface, remove the inline alert block (lines ~100–107 per the exploration report), and update any caller type if TypeScript flags it.

If the exploration misreported line numbers, read the file first to locate the block.

- [ ] **Step 3: Run the LinkGrabber tests**

Run: `npx vitest run src/views/LinkGrabberView`
Expected: PASS. If a test asserts on `errorMessage` rendering in the DOM, remove that assertion — error display moved to the global toast.

- [ ] **Step 4: Commit**

```bash
git add src/views/LinkGrabberView
git commit -m "refactor(link): replace local error state with global toast (#74)"
```

---

## Task 8: Migrate `useClipboardMonitoring`

**Files:**
- Modify: `src/hooks/useClipboardMonitoring.ts`

The current mutation has an inline `onSuccess` in `.mutate(...)` — this suppresses the hook-level onError registration because `useMutation` fires both. We'll add an `onSuccess` toast at the hook options level and let the default error toast do its job.

- [ ] **Step 1: Rewrite the mutation**

```ts
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import { toast } from '@/lib/toast';

export function useClipboardMonitoring(initial: boolean = false) {
  const { t } = useTranslation();
  const [isEnabled, setIsEnabled] = useState(initial);

  const toggleMutation = useTauriMutation<boolean, { enabled: boolean }>(
    'clipboard_toggle',
    {
      onSuccess: (confirmed) => {
        setIsEnabled(confirmed);
        toast.success(
          confirmed ? t('clipboard.toast.enabled') : t('clipboard.toast.disabled'),
        );
      },
    },
  );

  const toggle = useCallback(
    (enabled: boolean) => {
      toggleMutation.mutate({ enabled });
    },
    [toggleMutation],
  );

  return { isEnabled, toggle, isToggling: toggleMutation.isPending };
}
```

(Adapt initial signature to match how the hook is currently consumed — read the original file first and preserve its public shape.)

- [ ] **Step 2: Run tests touching clipboard**

Run: `npx vitest run src/hooks`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/hooks/useClipboardMonitoring.ts
git commit -m "feat(ui): surface clipboard toggle feedback via global toast (#74)"
```

---

## Task 9: Verification, changelog, and final sanity check

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Run the full Rust test suite (no Rust changes expected — regression check)**

Run: `cargo test --workspace`
Expected: PASS (untouched).

- [ ] **Step 2: Run the full frontend suite**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 3: Lint**

Run: `npx oxlint .`
Expected: zero errors. Fix by correcting the cause root (no eslint-disable).

- [ ] **Step 4: Verify no direct `sonner` import outside `src/lib/toast.ts` and `src/App.tsx`**

Run: Grep for `from "sonner"` in `src/`. Expected hits: only `src/lib/toast.ts` and `src/App.tsx`.

- [ ] **Step 5: Update `CHANGELOG.md`**

In `[Unreleased]`, under `Changed`:

```
- UI: every IPC mutation now surfaces an error toast by default via `useTauriMutation`; migrated all call sites (downloads, settings, plugins, link grabber, clipboard) to rely on this default. Inline error state removed from the link grabber. (#74)
```

Under `Added`:

```
- `useTauriMutation` now accepts `silentError` and `errorMessage` options for explicit opt-out or message mapping. (#74)
```

- [ ] **Step 6: Commit changelog**

```bash
git add CHANGELOG.md
git commit -m "docs(ui): changelog entry for global toast feedback (#74)"
```

- [ ] **Step 7: Adversarial review**

Dispatch a code-reviewer subagent to audit the diff vs the acceptance criteria in issue #74. Capture findings, then fix or justify each one before opening a PR.

---

## Acceptance criteria mapping

- [ ] Every failed IPC mutation produces a sonner toast unless explicit opt-out → Task 1 (default) + Tasks 4–8 (remove custom suppressors)
- [ ] No custom inline error UI state remains outside form-validation cases → Task 7 (LinkGrabberView)
- [ ] All `*.toast.*` i18n keys present in `fr.json` AND `en.json` → Task 3
- [ ] Tests green (`cargo test --workspace` + `npx vitest run`) → Task 9
- [ ] Lint green (`npx oxlint .`) → Task 9
- [ ] `src/lib/toast.ts` remains the public API (no direct sonner import elsewhere) → Task 9, step 4
- [ ] `silentError` opt-out available → Task 1
