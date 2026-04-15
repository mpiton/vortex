import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { PluginStoreEntry } from "@/types/plugin-store";

const STORE_QUERY_KEY = ["plugin_store_list"] as const;

export function usePluginStore() {
  const queryClient = useQueryClient();
  const [installingNames, setInstallingNames] = useState<Set<string>>(new Set());
  const [updatingNames, setUpdatingNames] = useState<Set<string>>(new Set());

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
    onMutate: (name) => setInstallingNames((s) => new Set(s).add(name)),
    onSettled: (_, __, name) =>
      setInstallingNames((s) => {
        const next = new Set(s);
        next.delete(name);
        return next;
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STORE_QUERY_KEY }),
  });

  const updateMutation = useMutation({
    mutationFn: (name: string) => invoke<void>("plugin_store_update", { name }),
    onMutate: (name) => setUpdatingNames((s) => new Set(s).add(name)),
    onSettled: (_, __, name) =>
      setUpdatingNames((s) => {
        const next = new Set(s);
        next.delete(name);
        return next;
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STORE_QUERY_KEY }),
  });

  return {
    entries,
    isLoading,
    isError,
    refreshStore: () => refreshMutation.mutate(),
    installPlugin: (name: string) => installMutation.mutate(name),
    updatePlugin: (name: string) => updateMutation.mutate(name),
    isInstalling: (name: string) => installingNames.has(name),
    isUpdating: (name: string) => updatingNames.has(name),
    isRefreshing: refreshMutation.isPending,
  };
}
