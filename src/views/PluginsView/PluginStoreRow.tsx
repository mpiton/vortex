import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { PluginStoreEntry } from "@/types/plugin-store";

interface PluginStoreRowProps {
  entry: PluginStoreEntry;
  onInstall: (name: string) => void;
  onUpdate: (name: string) => void;
  onDisable?: (name: string) => void;
  onUninstall?: (name: string) => void;
  isInstalling: boolean;
  isUpdating: boolean;
}

export function PluginStoreRow({
  entry,
  onInstall,
  onUpdate,
  onDisable,
  onUninstall,
  isInstalling,
  isUpdating,
}: PluginStoreRowProps) {
  return (
    <div className="flex items-center gap-3 px-3 py-2.5 border-b border-border last:border-0 hover:bg-muted/30">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="text-sm font-medium text-foreground truncate">
            {entry.name}
          </span>
          {entry.official && (
            <Badge variant="secondary" className="text-xs shrink-0">
              officiel
            </Badge>
          )}
          <StatusBadge entry={entry} />
        </div>
        <p className="text-xs text-muted-foreground mt-0.5 truncate">
          {entry.description} · {entry.category} · {entry.author}
          {entry.installedVersion && entry.status === "update_available" && (
            <span className="ml-1">· installé&nbsp;: {entry.installedVersion}</span>
          )}
        </p>
      </div>

      <div className="flex items-center gap-1.5 shrink-0">
        {entry.status === "not_installed" && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => onInstall(entry.name)}
            disabled={isInstalling}
          >
            Installer
          </Button>
        )}
        {entry.status === "update_available" && (
          <Button
            size="sm"
            variant="outline"
            className="text-amber-500 border-amber-500 hover:bg-amber-500/10"
            onClick={() => onUpdate(entry.name)}
            disabled={isUpdating}
          >
            Mettre à jour
          </Button>
        )}
        {(entry.status === "installed" || entry.status === "update_available" || entry.status === "downgrade") && (
          <>
            {onDisable && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onDisable(entry.name)}
              >
                Désactiver
              </Button>
            )}
            {onUninstall && (
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:text-destructive"
                onClick={() => onUninstall(entry.name)}
              >
                Désinstaller
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function StatusBadge({ entry }: { entry: PluginStoreEntry }) {
  if (entry.status === "installed") {
    return (
      <Badge variant="outline" className="text-xs border-green-500 text-green-500 shrink-0">
        installe
      </Badge>
    );
  }
  if (entry.status === "update_available") {
    return (
      <Badge variant="outline" className="text-xs border-amber-500 text-amber-500 shrink-0">
        {entry.version} disponible
      </Badge>
    );
  }
  if (entry.status === "downgrade") {
    return (
      <Badge variant="outline" className="text-xs border-blue-500 text-blue-500 shrink-0">
        version locale plus recente
      </Badge>
    );
  }
  return null;
}
