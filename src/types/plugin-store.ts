export interface PluginStoreEntry {
  name: string;
  description: string;
  author: string;
  /** Version disponible dans le registre. */
  version: string;
  /** Version installée localement, si présente. */
  installedVersion: string | null;
  category: string;
  official: boolean;
  /** "not_installed" | "installed" | "update_available" | "downgrade" */
  status: "not_installed" | "installed" | "update_available" | "downgrade";
  /**
   * URL du dépôt GitHub depuis le registre. Vide si le plugin n'expose
   * pas d'URL — auquel cas l'action "Report broken plugin" est cachée.
   */
  repository: string;
}
