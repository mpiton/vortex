import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { tauriInvoke } from "@/api/client";
import { PluginStoreRow } from "./PluginsView/PluginStoreRow";
import { PluginsHeader } from "./PluginsView/PluginsHeader";
import { PluginsToolbar } from "./PluginsView/PluginsToolbar";
import { PluginConfigDialog } from "./PluginsView/PluginConfigDialog";
import { groupByCategory } from "./PluginsView/groupByCategory";
import { usePluginStore } from "./PluginsView/usePluginStore";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import type { PluginConfigView } from "@/types/plugin-config";
import type { PluginStoreEntry } from "@/types/plugin-store";

const CATEGORIES = [
  "all",
  "crawler",
  "hoster",
  "debrid",
  "container",
  "captcha",
  "extractor",
  "notifier",
  "utility",
];

const STORE_INVALIDATE_KEYS = [["plugin_store_list"]] as const;

function isInstalled(status: string): boolean {
  return status === "installed" || status === "update_available" || status === "downgrade";
}

export function PluginsView() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("all");
  // Session-local optimistic disabled state. PluginStoreEntryDto has no
  // "disabled" variant yet, so the refetch after plugin_disable still reports
  // the plugin as installed. We track disabled names here so the row can
  // render as inactive and expose "Enable" until the DTO grows a matching
  // field. State is not persisted across reloads on purpose — same as the
  // pre-PR behaviour.
  const [locallyDisabled, setLocallyDisabled] = useState<ReadonlySet<string>>(new Set());
  const [configPluginName, setConfigPluginName] = useState<string | null>(null);

  // Each installed plugin's schema is fetched once on mount so the
  // "Configure" button can be hidden when the manifest declares no
  // `[config]` fields. We piggyback on TanStack Query's cache so the
  // dialog can reuse the same key without a duplicate fetch.

  const {
    entries,
    isLoading,
    isError,
    refreshStore,
    installPlugin,
    updatePlugin,
    isInstalling,
    isUpdating,
    isRefreshing,
  } = usePluginStore();

  const disableMutation = useTauriMutation<void, { name: string }>("plugin_disable", {
    invalidateKeys: STORE_INVALIDATE_KEYS,
    onSuccess: (_data, variables) => {
      setLocallyDisabled((prev) => {
        const next = new Set(prev);
        next.add(variables.name);
        return next;
      });
      toast.success(t("plugins.toast.disableSuccess"));
    },
  });

  const enableMutation = useTauriMutation<void, { name: string }>("plugin_enable", {
    invalidateKeys: STORE_INVALIDATE_KEYS,
    onSuccess: (_data, variables) => {
      setLocallyDisabled((prev) => {
        const next = new Set(prev);
        next.delete(variables.name);
        return next;
      });
      toast.success(t("plugins.toast.enableSuccess"));
    },
  });

  const uninstallMutation = useTauriMutation<void, { name: string }>("plugin_uninstall", {
    invalidateKeys: STORE_INVALIDATE_KEYS,
    onSuccess: (_data, variables) => {
      setLocallyDisabled((prev) => {
        if (!prev.has(variables.name)) return prev;
        const next = new Set(prev);
        next.delete(variables.name);
        return next;
      });
      toast.success(t("plugins.toast.uninstallSuccess"));
    },
  });

  const reportBrokenMutation = useTauriMutation<
    string,
    { pluginName: string; logLines?: string[]; testedUrl?: string }
  >("plugin_report_broken", {
    onSuccess: (url, variables) => {
      // Best-effort clipboard copy so the user always has the URL even
      // if the OS launcher silently failed (no graphical session, broken
      // `xdg-open`, etc.).
      //
      // `navigator.clipboard` itself is undefined in non-secure contexts
      // and inside webviews that opt out of the API, so we must guard
      // before the call — accessing `.writeText` on `undefined` would
      // throw synchronously and break the success toast.
      const clipboard = navigator.clipboard;
      if (clipboard?.writeText) {
        void clipboard
          .writeText(url)
          .then(() => {
            toast.success(
              t("plugins.toast.reportBrokenSuccessWithCopy", { name: variables.pluginName }),
            );
          })
          .catch(() => {
            toast.success(t("plugins.toast.reportBrokenSuccess", { name: variables.pluginName }));
          });
      } else {
        toast.success(t("plugins.toast.reportBrokenSuccess", { name: variables.pluginName }));
      }
    },
    onError: (error, variables) => {
      toast.error(
        t("plugins.toast.reportBrokenError", {
          name: variables.pluginName,
          reason: error.message,
        }),
      );
    },
  });

  // The frontend short-circuits the action for plugins whose registry
  // entry carries no GitHub repository: the backend would only return
  // `Validation` and the menu item would advertise an action that
  // never works. Limiting the test to `github.com` mirrors the domain
  // validation in `parse_github_owner_repo`.
  const canReportBroken = (entry: PluginStoreEntry): boolean => {
    const repo = entry.repository ?? "";
    return repo.startsWith("https://github.com/") || repo.startsWith("http://github.com/");
  };

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return entries.filter((e) => {
      const matchSearch =
        query.length === 0 ||
        e.name.toLowerCase().includes(query) ||
        e.description.toLowerCase().includes(query) ||
        e.author.toLowerCase().includes(query);
      const matchCategory = category === "all" || e.category === category;
      return matchSearch && matchCategory;
    });
  }, [entries, search, category]);

  const enabledCount = useMemo(
    () => entries.filter((e) => isInstalled(e.status) && !locallyDisabled.has(e.name)).length,
    [entries, locallyDisabled],
  );

  const installedNames = useMemo(
    () =>
      entries
        .filter((e) => isInstalled(e.status) && !locallyDisabled.has(e.name))
        .map((e) => e.name),
    [entries, locallyDisabled],
  );

  const { data: configsByPlugin } = useQuery({
    queryKey: ["plugin_config_get_all", installedNames],
    enabled: installedNames.length > 0,
    queryFn: async () => {
      const results = await Promise.all(
        installedNames.map(async (name) => {
          try {
            const view = await tauriInvoke<PluginConfigView>("plugin_config_get", { name });
            return [name, view] as const;
          } catch {
            return [name, null] as const;
          }
        }),
      );
      return Object.fromEntries(results) as Record<string, PluginConfigView | null>;
    },
  });

  const hasConfig = (name: string): boolean => {
    const view = configsByPlugin?.[name];
    return view !== null && view !== undefined && view.fields.length > 0;
  };

  const groups = useMemo(() => groupByCategory(filtered), [filtered]);

  return (
    <div className="flex flex-col h-full bg-surface-alt">
      <PluginsHeader
        enabledCount={enabledCount}
        onRefresh={refreshStore}
        isRefreshing={isRefreshing}
      />
      <PluginsToolbar
        categories={CATEGORIES}
        activeCategory={category}
        onCategoryChange={setCategory}
        search={search}
        onSearchChange={setSearch}
      />

      <div className="flex-1 overflow-y-auto">
        {isLoading && (
          <div className="flex items-center justify-center h-32 text-text-dim text-xs">
            {t("plugins.loading")}
          </div>
        )}

        {isError && (
          <div className="flex flex-col items-center justify-center h-32 gap-2">
            <p className="text-xs text-text-dim">{t("plugins.error")}</p>
            <Button variant="outline" size="sm" onClick={refreshStore}>
              {t("plugins.retry")}
            </Button>
          </div>
        )}

        {!isLoading && !isError && filtered.length === 0 && (
          <div className="flex items-center justify-center h-32 text-text-dim text-xs">
            {t("plugins.empty")}
          </div>
        )}

        {!isLoading &&
          !isError &&
          groups.map((group) => {
            const label = t(`plugins.categories.${group.category}`, {
              defaultValue: group.category,
            });
            return (
              <section key={group.category}>
                <h3 className="text-[11px] font-semibold text-text-dim uppercase tracking-widest px-6 pt-4 pb-1.5">
                  {t("plugins.group.count", {
                    label,
                    count: group.entries.length,
                  })}
                </h3>
                <div className="mx-6 mt-1.5 bg-surface border border-border-soft rounded-lg overflow-hidden">
                  {group.entries.map((entry) => (
                    <PluginStoreRow
                      key={entry.name}
                      entry={entry}
                      isLocallyDisabled={locallyDisabled.has(entry.name)}
                      onInstall={installPlugin}
                      onUpdate={updatePlugin}
                      onDisable={(name) => disableMutation.mutate({ name })}
                      onEnable={(name) => enableMutation.mutate({ name })}
                      onUninstall={(name) => uninstallMutation.mutate({ name })}
                      onConfigure={(name) => setConfigPluginName(name)}
                      onReportBroken={
                        canReportBroken(entry)
                          ? (name) => reportBrokenMutation.mutate({ pluginName: name })
                          : undefined
                      }
                      hasConfig={hasConfig(entry.name)}
                      isInstalling={isInstalling(entry.name)}
                      isUpdating={isUpdating(entry.name)}
                    />
                  ))}
                </div>
              </section>
            );
          })}

        <div className="h-6" />
      </div>

      <PluginConfigDialog
        pluginName={configPluginName}
        open={configPluginName !== null}
        onOpenChange={(open) => {
          if (!open) setConfigPluginName(null);
        }}
      />
    </div>
  );
}
