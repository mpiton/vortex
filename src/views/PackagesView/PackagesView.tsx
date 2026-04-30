import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useTauriMutation } from "@/api/hooks";
import { packageQueries, downloadQueries } from "@/api/queries";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { usePackagesQuery, usePackageDownloadsQuery } from "@/hooks/usePackagesQuery";
import { toast } from "@/lib/toast";
import type { DownloadView } from "@/types/download";
import type {
  CreatePackageInput,
  PackagePatch,
  PackageSourceType,
  PackageView,
} from "@/types/package";
import {
  AddPackageDialog,
  DeletePackageDialog,
  FolderDialog,
  PasswordDialog,
  RenamePackageDialog,
} from "./PackageDialogs";
import { PackageTree } from "./PackageTree";
import type { PackageRowActions } from "./PackageRow";
import { PackageToolbar } from "./PackageToolbar";

const INVALIDATE_KEYS = [packageQueries.all()] as const;
const INVALIDATE_KEYS_WITH_DOWNLOADS = [
  packageQueries.all(),
  downloadQueries.all(),
] as const;

interface PackageMoveOutcome {
  moved: number[];
  failed: Array<{ id: number; reason: string }>;
}

export function PackagesView() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [filter, setFilter] = useState<"all" | PackageSourceType>("all");
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search, 300);

  const [addOpen, setAddOpen] = useState(false);
  const [renaming, setRenaming] = useState<PackageView | null>(null);
  const [passwordTarget, setPasswordTarget] = useState<PackageView | null>(null);
  const [folderTarget, setFolderTarget] = useState<PackageView | null>(null);
  const [deleting, setDeleting] = useState<PackageView | null>(null);

  const [expandedId, setExpandedId] = useState<string | null>(null);
  const dragRef = useRef<{ downloadId: number; fromPackageId: string } | null>(null);

  const queryFilter = useMemo(() => {
    const f: { sourceType?: string; nameQ?: string } = {};
    if (filter !== "all") f.sourceType = filter;
    if (debouncedSearch.trim().length > 0) f.nameQ = debouncedSearch.trim();
    return Object.keys(f).length > 0 ? f : undefined;
  }, [filter, debouncedSearch]);

  const { data, isLoading, error } = usePackagesQuery(queryFilter);
  const packages = useMemo<PackageView[]>(() => data ?? [], [data]);

  const {
    data: childrenData,
    isLoading: childrenLoading,
    error: childrenError,
  } = usePackageDownloadsQuery(expandedId);
  const childrenById = useMemo<DownloadView[] | null>(
    () => (expandedId ? childrenData ?? null : null),
    [expandedId, childrenData],
  );

  const invalidatePackages = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: packageQueries.all() });
  }, [queryClient]);

  const createMut = useTauriMutation<string, CreatePackageInput & Record<string, unknown>>(
    "package_create",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("packages.toast.createError"),
    },
  );

  const updateMut = useTauriMutation<void, { id: string; patch: PackagePatch } & Record<string, unknown>>(
    "package_update",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("packages.toast.updateError"),
    },
  );

  const deleteMut = useTauriMutation<void, { id: string; deleteDownloads: boolean }>(
    "package_delete",
    {
      invalidateKeys: INVALIDATE_KEYS_WITH_DOWNLOADS,
      errorMessage: () => t("packages.toast.deleteError"),
    },
  );

  const passwordMut = useTauriMutation<void, { id: string; password: string | null }>(
    "package_set_password",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("packages.toast.passwordError"),
    },
  );

  const priorityMut = useTauriMutation<void, { id: string; priority: number }>(
    "package_set_priority",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("packages.toast.updateError"),
    },
  );

  const moveFolderMut = useTauriMutation<
    PackageMoveOutcome,
    { id: string; newFolder: string }
  >("package_move_to_folder", {
    invalidateKeys: INVALIDATE_KEYS_WITH_DOWNLOADS,
    errorMessage: () => t("packages.toast.moveError"),
  });

  const toggleAutoExtractMut = useTauriMutation<boolean, { id: string }>(
    "package_toggle_auto_extract",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("packages.toast.updateError"),
    },
  );

  const removeFromPackageMut = useTauriMutation<void, { packageId: string; downloadId: number }>(
    "package_remove_download",
    { silentError: true },
  );

  const addToPackageMut = useTauriMutation<void, { packageId: string; downloadId: number }>(
    "package_add_download",
    { silentError: true },
  );

  const pauseMut = useTauriMutation<unknown, { id: number }>("download_pause", {
    invalidateKeys: [downloadQueries.all()] as const,
    silentError: true,
  });

  const resumeMut = useTauriMutation<unknown, { id: number }>("download_resume", {
    invalidateKeys: [downloadQueries.all()] as const,
    silentError: true,
  });

  const handleCreate = useCallback(
    async (input: CreatePackageInput) => {
      await createMut.mutateAsync({
        name: input.name,
        sourceType: input.sourceType,
        folderPath: input.folderPath,
      });
      toast.success(t("packages.toast.createSuccess"));
    },
    [createMut, t],
  );

  const handleRename = useCallback(
    async (name: string) => {
      if (!renaming) return;
      await updateMut.mutateAsync({ id: renaming.id, patch: { name } });
      toast.success(t("packages.toast.updateSuccess"));
    },
    [renaming, updateMut, t],
  );

  const handleSetPassword = useCallback(
    async (password: string | null) => {
      if (!passwordTarget) return;
      await passwordMut.mutateAsync({ id: passwordTarget.id, password });
      toast.success(t("packages.toast.passwordSuccess"));
    },
    [passwordTarget, passwordMut, t],
  );

  const handleChangeFolder = useCallback(
    async (newFolder: string) => {
      if (!folderTarget) return;
      const outcome = await moveFolderMut.mutateAsync({ id: folderTarget.id, newFolder });
      const moved = outcome?.moved.length ?? 0;
      const failed = outcome?.failed.length ?? 0;
      if (failed > 0) {
        toast.error(
          t("packages.toast.movePartialError", { moved, failed, total: moved + failed }),
        );
      } else {
        toast.success(t("packages.toast.moveSuccess", { count: moved }));
      }
    },
    [folderTarget, moveFolderMut, t],
  );

  const handleDelete = useCallback(
    async (deleteDownloads: boolean) => {
      if (!deleting) return;
      await deleteMut.mutateAsync({ id: deleting.id, deleteDownloads });
      toast.success(t("packages.toast.deleteSuccess"));
    },
    [deleting, deleteMut, t],
  );

  const pickFolder = useCallback(async () => {
    const picked = await openDialog({
      directory: true,
      multiple: false,
    }).catch(() => null);
    if (typeof picked === "string") return picked;
    return null;
  }, []);

  const fanoutDownloadAction = useCallback(
    async (downloads: DownloadView[], action: (id: number) => Promise<unknown>) => {
      const ids = downloads
        .map((d) => Number(d.id))
        .filter((n) => Number.isFinite(n));
      const results = await Promise.allSettled(ids.map((id) => action(id)));
      const failed = results.filter((r) => r.status === "rejected").length;
      return { total: ids.length, failed };
    },
    [],
  );

  const actions = useMemo<PackageRowActions>(() => ({
    toggleExpand: (id: string) => {
      setExpandedId((prev) => (prev === id ? null : id));
    },
    rename: (pkg) => setRenaming(pkg),
    setPassword: (pkg) => setPasswordTarget(pkg),
    changeFolder: (pkg) => setFolderTarget(pkg),
    deletePackage: (pkg) => setDeleting(pkg),
    toggleAutoExtract: (pkg) => {
      toggleAutoExtractMut.mutate(
        { id: pkg.id },
        { onSuccess: () => toast.success(t("packages.toast.updateSuccess")) },
      );
    },
    setPriority: (pkg, priority) => {
      priorityMut.mutate(
        { id: pkg.id, priority },
        { onSuccess: () => toast.success(t("packages.toast.updateSuccess")) },
      );
    },
    pauseAll: async (_pkg, downloads) => {
      const { failed } = await fanoutDownloadAction(downloads, (id) =>
        pauseMut.mutateAsync({ id }),
      );
      if (failed > 0) {
        toast.error(t("packages.toast.bulkActionError"));
      } else {
        toast.success(t("packages.toast.bulkPauseSuccess"));
      }
    },
    startAll: async (_pkg, downloads) => {
      const { failed } = await fanoutDownloadAction(downloads, (id) =>
        resumeMut.mutateAsync({ id }),
      );
      if (failed > 0) {
        toast.error(t("packages.toast.bulkActionError"));
      } else {
        toast.success(t("packages.toast.bulkStartSuccess"));
      }
    },
    beginDragDownload: (download, fromPackageId) => {
      const numericId = Number(download.id);
      if (!Number.isFinite(numericId)) return;
      dragRef.current = { downloadId: numericId, fromPackageId };
    },
    dropDownload: async (toPackageId, e) => {
      const transfer = e.dataTransfer;
      const rawId =
        transfer?.getData("application/x-vortex-download") ??
        String(dragRef.current?.downloadId ?? "");
      const fromId =
        transfer?.getData("application/x-vortex-source-package") ??
        dragRef.current?.fromPackageId ??
        "";
      const downloadId = Number(rawId);
      dragRef.current = null;
      if (!Number.isFinite(downloadId) || fromId === toPackageId || fromId === "") {
        return;
      }
      try {
        await removeFromPackageMut.mutateAsync({ packageId: fromId, downloadId });
        try {
          await addToPackageMut.mutateAsync({ packageId: toPackageId, downloadId });
        } catch (addError) {
          try {
            await addToPackageMut.mutateAsync({ packageId: fromId, downloadId });
          } catch {
            toast.error(t("packages.toast.moveDownloadRollbackError"));
            invalidatePackages();
            throw addError;
          }
          throw addError;
        }
        toast.success(t("packages.toast.moveDownloadSuccess"));
        invalidatePackages();
      } catch {
        toast.error(t("packages.toast.moveDownloadError"));
      }
    },
  }), [
    addToPackageMut,
    fanoutDownloadAction,
    invalidatePackages,
    pauseMut,
    priorityMut,
    removeFromPackageMut,
    resumeMut,
    t,
    toggleAutoExtractMut,
  ]);

  return (
    <div
      className="flex h-full min-h-0 flex-col gap-3 p-4"
      data-testid="packages-view"
    >
      <h1 className="text-2xl font-semibold">{t("packages.title")}</h1>
      <PackageToolbar
        filter={filter}
        onFilterChange={setFilter}
        search={search}
        onSearchChange={setSearch}
        onAddClick={() => setAddOpen(true)}
      />
      {error && (
        <div
          data-testid="packages-error"
          className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive"
        >
          {error.message}
        </div>
      )}
      {isLoading ? (
        <div
          data-testid="packages-loading"
          className="flex h-32 items-center justify-center text-sm text-muted-foreground"
        >
          {t("packages.loading")}
        </div>
      ) : (
        <PackageTree
          packages={packages}
          expandedId={expandedId}
          childrenLoading={childrenLoading}
          childrenError={(childrenError as Error | null) ?? null}
          childrenById={childrenById}
          actions={actions}
        />
      )}
      <AddPackageDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSubmit={handleCreate}
      />
      <RenamePackageDialog
        pkg={renaming}
        onCancel={() => setRenaming(null)}
        onSubmit={handleRename}
      />
      <PasswordDialog
        pkg={passwordTarget}
        onCancel={() => setPasswordTarget(null)}
        onSubmit={handleSetPassword}
      />
      <FolderDialog
        pkg={folderTarget}
        onCancel={() => setFolderTarget(null)}
        onPickFolder={pickFolder}
        onSubmit={handleChangeFolder}
      />
      <DeletePackageDialog
        pkg={deleting}
        onCancel={() => setDeleting(null)}
        onConfirm={handleDelete}
      />
    </div>
  );
}
