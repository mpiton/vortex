import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import type { PluginStoreEntry } from "@/types/plugin-store";

const STORE_QUERY_KEY = ["plugin_store_list"] as const;

export function usePluginStore() {
  const { t } = useTranslation();
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
