import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { PluginStoreEntry } from "@/types/plugin-store";

const STORE_QUERY_KEY = ["plugin_store_list"] as const;

export function usePluginStore() {
  const queryClient = useQueryClient();

  const { data: entries = [], isLoading, isError } = useQuery({
    queryKey: STORE_QUERY_KEY,
    queryFn: () => invoke<PluginStoreEntry[]>("plugin_store_list"),
  });

  const refreshMutation = useMutation({
    mutationFn: () => invoke<void>("plugin_store_refresh"),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STORE_QUERY_KEY }),
  });

  const installMutation = useMutation({
    mutationFn: (name: string) => invoke<void>("plugin_store_install", { name }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STORE_QUERY_KEY }),
  });

  const updateMutation = useMutation({
    mutationFn: (name: string) => invoke<void>("plugin_store_update", { name }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STORE_QUERY_KEY }),
  });

  return {
    entries,
    isLoading,
    isError,
    refreshStore: () => refreshMutation.mutate(),
    installPlugin: (name: string) => installMutation.mutate(name),
    updatePlugin: (name: string) => updateMutation.mutate(name),
    isInstalling: installMutation.isPending,
    isUpdating: updateMutation.isPending,
    isRefreshing: refreshMutation.isPending,
    installingName: installMutation.variables ?? null,
    updatingName: updateMutation.variables ?? null,
  };
}
