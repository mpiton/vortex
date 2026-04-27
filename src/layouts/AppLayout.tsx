import { useEffect } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { SkipLink } from "@/components/a11y/SkipLink";
import { ROUTES } from "@/types/layout";
import { useDownloadProgress } from "@/hooks/useDownloadProgress";
import { useDownloadEvents } from "@/hooks/useDownloadEvents";
import { useAppEffects } from "@/hooks/useAppEffects";
import { tauriInvoke } from "@/api/client";
import { useTauriQuery } from "@/api/hooks";
import { downloadQueries } from "@/api/queries";
import { useDownloadStore } from "@/stores/downloadStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useUiStore } from "@/stores/uiStore";
import { SHORTCUT_ACTIONS, dispatchShortcutAction } from "@/lib/keyboardShortcuts";
import { isMacPlatform } from "@/lib/platform";
import { toast } from "@/lib/toast";
import type { AppConfig } from "@/types/settings";

export function AppLayout() {
  const navigate = useNavigate();
  const location = useLocation();
  const { t, i18n } = useTranslation();
  const setConfig = useSettingsStore((s) => s.setConfig);
  const updateCountByState = useDownloadStore((s) => s.updateCountByState);
  useDownloadProgress();
  useDownloadEvents();
  useAppEffects();

  const { data: config } = useTauriQuery<AppConfig>("settings_get", undefined, {
    queryKey: ["settings_get"],
    staleTime: 30_000,
  });

  const { data: countByState } = useTauriQuery<Record<string, number>>(
    "download_count_by_state",
    undefined,
    { queryKey: downloadQueries.countByState(), staleTime: 5_000 },
  );

  useEffect(() => {
    if (config) {
      setConfig(config);
    }
  }, [config, setConfig]);

  useEffect(() => {
    if (countByState) {
      updateCountByState(countByState);
    }
  }, [countByState, updateCountByState]);

  useEffect(() => {
    if (!config?.locale) return;

    const baseLocale = config.locale.split("-")[0];
    const supportedLocales = Object.keys(i18n.store?.data ?? i18n.options?.resources ?? {});
    const nextLocale =
      supportedLocales.length > 0
        ? supportedLocales.includes(baseLocale)
          ? baseLocale
          : "en"
        : baseLocale;

    if (i18n.resolvedLanguage === nextLocale || i18n.language === nextLocale) {
      return;
    }

    void i18n.changeLanguage(nextLocale);
  }, [config?.locale, i18n]);

  useEffect(() => {
    function isEditableTarget(target: EventTarget | null) {
      return (
        target instanceof HTMLElement &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
      );
    }

    function handleKeydown(event: KeyboardEvent) {
      if (event.key === "Escape" && useUiStore.getState().detailsPanelOpen) {
        event.preventDefault();
        useUiStore.getState().setDetailsPanelOpen(false);
        return;
      }

      const modifier = isMacPlatform() ? event.metaKey : event.ctrlKey;
      if (!modifier) {
        if (isEditableTarget(event.target)) return;

        if (location.pathname === "/downloads" && (event.key === " " || event.code === "Space")) {
          event.preventDefault();
          dispatchShortcutAction(SHORTCUT_ACTIONS.downloadsToggleSelected);
          return;
        }

        if (
          location.pathname === "/downloads" &&
          (event.key === "Delete" || event.key === "Backspace")
        ) {
          event.preventDefault();
          dispatchShortcutAction(SHORTCUT_ACTIONS.downloadsRemoveSelected);
        }
        return;
      }

      if (isEditableTarget(event.target)) return;

      const lowerKey = event.key.toLowerCase();

      if (event.shiftKey && lowerKey === "p") {
        event.preventDefault();
        const currentConfig = useSettingsStore.getState().config;
        const nextEnabled = !(currentConfig?.clipboardMonitoring ?? false);
        void tauriInvoke<boolean>("clipboard_toggle", { enabled: nextEnabled }).then(
          (confirmed) => {
            const latestConfig = useSettingsStore.getState().config;
            if (latestConfig) {
              setConfig({ ...latestConfig, clipboardMonitoring: confirmed });
            }
          },
        );
        return;
      }

      if (lowerKey === "v" && !event.shiftKey && !event.altKey) {
        event.preventDefault();
        navigator.clipboard
          .readText()
          .then((text) => {
            if (!text) return;
            void navigate("/link-grabber", {
              replace: location.pathname === "/link-grabber",
              state: {
                focusPaste: true,
                pasteContent: text,
                pasteToken: `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`,
              },
            });
          })
          .catch(() => {
            toast.error(t("linkGrabber.toast.clipboardReadFailed"));
          });
        return;
      }

      if (lowerKey === "f" && location.pathname === "/downloads") {
        event.preventDefault();
        dispatchShortcutAction(SHORTCUT_ACTIONS.downloadsFocusSearch);
        return;
      }

      if (lowerKey === "n") {
        event.preventDefault();
        void navigate("/link-grabber", {
          replace: location.pathname === "/link-grabber",
          state: { focusPaste: true },
        });
        return;
      }

      if (lowerKey === "a" && location.pathname === "/downloads") {
        event.preventDefault();
        dispatchShortcutAction(SHORTCUT_ACTIONS.downloadsSelectAll);
        return;
      }

      if (event.key === ",") {
        event.preventDefault();
        void navigate("/settings");
        return;
      }

      const index = parseInt(event.key, 10);
      if (index >= 1 && index <= 9 && ROUTES[index - 1]) {
        event.preventDefault();
        void navigate(ROUTES[index - 1].path);
      }
    }

    window.addEventListener("keydown", handleKeydown);
    return () => window.removeEventListener("keydown", handleKeydown);
  }, [location.pathname, navigate, setConfig, t]);

  return (
    <div className="flex h-screen w-screen overflow-hidden font-mono text-[13px] leading-normal text-text">
      <SkipLink />
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <main
          id="main-content"
          tabIndex={-1}
          className="flex-1 overflow-y-auto bg-surface-alt focus:outline-none"
        >
          <Outlet />
        </main>
        <StatusBar />
      </div>
    </div>
  );
}
