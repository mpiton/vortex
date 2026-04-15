import { useState } from "react";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { PluginStoreRow } from "./PluginsView/PluginStoreRow";
import { usePluginStore } from "./PluginsView/usePluginStore";
import { useTauriMutation } from "@/api/hooks";

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

export function PluginsView() {
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("all");

  const {
    entries,
    isLoading,
    isError,
    refreshStore,
    installPlugin,
    updatePlugin,
    isInstalling,
    isUpdating,
    installingName,
    updatingName,
    isRefreshing,
  } = usePluginStore();

  const disableMutation = useTauriMutation<void, { name: string }>(
    "plugin_disable",
    { invalidateKeys: STORE_INVALIDATE_KEYS },
  );

  const uninstallMutation = useTauriMutation<void, { name: string }>(
    "plugin_uninstall",
    { invalidateKeys: STORE_INVALIDATE_KEYS },
  );

  const filtered = entries.filter((e) => {
    const matchSearch =
      search.length === 0 ||
      e.name.toLowerCase().includes(search.toLowerCase()) ||
      e.description.toLowerCase().includes(search.toLowerCase()) ||
      e.author.toLowerCase().includes(search.toLowerCase());
    const matchCat = category === "all" || e.category === category;
    return matchSearch && matchCat;
  });

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 p-3 border-b border-border">
        <Input
          placeholder="Rechercher un plugin..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="flex-1"
        />
        <Select value={category} onValueChange={setCategory}>
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {CATEGORIES.map((c) => (
              <SelectItem key={c} value={c}>
                {c === "all" ? "Toutes catégories" : c}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="icon"
          onClick={refreshStore}
          disabled={isRefreshing}
          title="Rafraîchir le catalogue"
        >
          {isRefreshing ? "…" : "↻"}
        </Button>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {isLoading && (
          <div className="flex items-center justify-center h-32 text-muted-foreground text-sm">
            Chargement du catalogue…
          </div>
        )}
        {isError && (
          <div className="flex flex-col items-center justify-center h-32 gap-2">
            <p className="text-sm text-muted-foreground">Erreur de chargement</p>
            <Button variant="outline" size="sm" onClick={refreshStore}>
              Réessayer
            </Button>
          </div>
        )}
        {!isLoading && !isError && filtered.length === 0 && (
          <div className="flex items-center justify-center h-32 text-muted-foreground text-sm">
            Aucun plugin trouvé
          </div>
        )}
        {!isLoading &&
          filtered.map((entry) => (
            <PluginStoreRow
              key={entry.name}
              entry={entry}
              onInstall={installPlugin}
              onUpdate={updatePlugin}
              onDisable={(name) => disableMutation.mutate({ name })}
              onUninstall={(name) => uninstallMutation.mutate({ name })}
              isInstalling={isInstalling && installingName === entry.name}
              isUpdating={isUpdating && updatingName === entry.name}
            />
          ))}
      </div>
    </div>
  );
}
