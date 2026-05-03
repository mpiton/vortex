import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import type { PluginStoreEntry } from "@/types/plugin-store";

const STORE_QUERY_KEY = ["plugin_store_list"] as const;

// Tracks the count of concurrent operations per plugin name. A Set would lose
// multiplicity when the same plugin is acted on twice: the first onSettled
// would clear the flag while a second request is still in-flight.
function incrementCounter(prev: ReadonlyMap<string, number>, name: string): Map<string, number> {
  const next = new Map(prev);
  next.set(name, (next.get(name) ?? 0) + 1);
  return next;
}

function decrementCounter(prev: ReadonlyMap<string, number>, name: string): Map<string, number> {
  const next = new Map(prev);
  const current = next.get(name) ?? 0;
  if (current <= 1) {
    next.delete(name);
  } else {
    next.set(name, current - 1);
  }
  return next;
}

export function usePluginStore() {
  const { t } = useTranslation();
  const [installingCounts, setInstallingCounts] = useState<ReadonlyMap<string, number>>(new Map());
  const [updatingCounts, setUpdatingCounts] = useState<ReadonlyMap<string, number>>(new Map());

  const {
    data: entries = [],
    isLoading,
    isError,
  } = useQuery({
    queryKey: STORE_QUERY_KEY,
    queryFn: () => invoke<PluginStoreEntry[]>("plugin_store_list"),
  });

  const refreshMutation = useTauriMutation<void, void>("plugin_store_refresh", {
    invalidateKeys: [STORE_QUERY_KEY],
    onSuccess: () => toast.success(t("plugins.toast.refreshSuccess")),
  });

  const installMutation = useTauriMutation<void, { name: string }>("plugin_store_install", {
    invalidateKeys: [STORE_QUERY_KEY],
    onMutate: (variables) => {
      setInstallingCounts((prev) => incrementCounter(prev, variables.name));
    },
    onSuccess: (_data, variables) => {
      toast.success(t("plugins.toast.installSuccess", { name: variables.name }));
    },
    onSettled: (_data, _error, variables) => {
      setInstallingCounts((prev) => decrementCounter(prev, variables.name));
    },
  });

  const updateMutation = useTauriMutation<void, { name: string }>("plugin_store_update", {
    invalidateKeys: [STORE_QUERY_KEY],
    onMutate: (variables) => {
      setUpdatingCounts((prev) => incrementCounter(prev, variables.name));
    },
    onSuccess: (_data, variables) => {
      toast.success(t("plugins.toast.updateSuccess", { name: variables.name }));
    },
    onSettled: (_data, _error, variables) => {
      setUpdatingCounts((prev) => decrementCounter(prev, variables.name));
    },
  });

  return {
    entries,
    isLoading,
    isError,
    refreshStore: () => refreshMutation.mutate(),
    installPlugin: (name: string) => installMutation.mutate({ name }),
    updatePlugin: (name: string) => updateMutation.mutate({ name }),
    isInstalling: (name: string) => (installingCounts.get(name) ?? 0) > 0,
    isUpdating: (name: string) => (updatingCounts.get(name) ?? 0) > 0,
    isRefreshing: refreshMutation.isPending,
  };
}
