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
  /** "not_installed" | "installed" | "update_available" */
  status: "not_installed" | "installed" | "update_available";
}
