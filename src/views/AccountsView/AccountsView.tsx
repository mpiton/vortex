import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { tauriInvoke } from "@/api/client";
import { useTauriMutation } from "@/api/hooks";
import { accountQueries } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { useAccountsQuery } from "@/hooks/useAccountsQuery";
import { toast } from "@/lib/toast";
import { useSettingsStore } from "@/stores/settingsStore";
import type {
  AccountPatch,
  AccountType,
  AccountView,
  AddAccountInput,
  ExportAccountsResult,
  ImportAccountsResult,
  ValidationOutcome,
} from "@/types/account";
import { AccountList } from "./AccountList";
import type { AccountRowActions } from "./AccountRow";
import { AddAccountDialog } from "./AddAccountDialog";
import { DeleteAccountDialog } from "./DeleteAccountDialog";
import { EditAccountDialog } from "./EditAccountDialog";
import { ExportAccountsDialog, ImportAccountsDialog } from "./ImportExportDialog";

const FILTER_ORDER: ReadonlyArray<"all" | AccountType> = ["all", "debrid", "premium", "free"];
const INVALIDATE_KEYS = [accountQueries.all()] as const;

export function AccountsView() {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<"all" | AccountType>("all");
  const [addOpen, setAddOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [deleting, setDeleting] = useState<AccountView | null>(null);
  const [editing, setEditing] = useState<AccountView | null>(null);
  const [validatingIds, setValidatingIds] = useState<ReadonlySet<string>>(() => new Set());

  const confirmBeforeDelete = useSettingsStore((s) => s.config?.confirmDelete ?? true);
  const queryClient = useQueryClient();

  const invalidateAccountsList = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: accountQueries.all() });
  }, [queryClient]);

  const { data, isLoading, error } = useAccountsQuery();
  const accounts = useMemo<AccountView[]>(() => data ?? [], [data]);

  const counts = useMemo(() => {
    const all = accounts.length;
    let debrid = 0;
    let premium = 0;
    let free = 0;
    for (const a of accounts) {
      if (a.accountType === "debrid") debrid++;
      else if (a.accountType === "premium") premium++;
      else if (a.accountType === "free") free++;
    }
    return { all, debrid, premium, free };
  }, [accounts]);

  const filteredAccounts = useMemo(() => {
    if (filter === "all") return accounts;
    return accounts.filter((a) => a.accountType === filter);
  }, [accounts, filter]);

  const addMut = useTauriMutation<string, AddAccountInput & Record<string, unknown>>(
    "account_add",
    {
      invalidateKeys: INVALIDATE_KEYS,
      errorMessage: () => t("accounts.toast.addError"),
    },
  );

  const updateMut = useTauriMutation<
    void,
    { id: string; patch: AccountPatch } & Record<string, unknown>
  >("account_update", {
    invalidateKeys: INVALIDATE_KEYS,
    errorMessage: () => t("accounts.toast.updateError"),
  });

  const deleteMut = useTauriMutation<void, { id: string }>("account_delete", {
    invalidateKeys: INVALIDATE_KEYS,
    errorMessage: () => t("accounts.toast.deleteError"),
  });

  const handleAddSubmit = useCallback(
    async (input: AddAccountInput) => {
      await addMut.mutateAsync({
        serviceName: input.serviceName,
        username: input.username,
        password: input.password,
        accountType: input.accountType,
      });
      toast.success(t("accounts.toast.addSuccess"));
    },
    [addMut, t],
  );

  const handleToggleEnabled = useCallback(
    (account: AccountView, nextEnabled: boolean) => {
      updateMut.mutate(
        { id: account.id, patch: { enabled: nextEnabled } },
        { onSuccess: () => toast.success(t("accounts.toast.updateSuccess")) },
      );
    },
    [updateMut, t],
  );

  const handleEdit = useCallback((account: AccountView) => {
    setEditing(account);
  }, []);

  const handleEditSubmit = useCallback(
    async (patch: AccountPatch) => {
      if (!editing) return;
      await updateMut.mutateAsync({ id: editing.id, patch });
      toast.success(t("accounts.toast.updateSuccess"));
    },
    [editing, updateMut, t],
  );

  const requestDelete = useCallback(
    (account: AccountView) => {
      if (confirmBeforeDelete) {
        setDeleting(account);
      } else {
        deleteMut.mutate(
          { id: account.id },
          { onSuccess: () => toast.success(t("accounts.toast.deleteSuccess")) },
        );
      }
    },
    [confirmBeforeDelete, deleteMut, t],
  );

  const confirmDelete = useCallback(() => {
    if (!deleting) return;
    deleteMut.mutate(
      { id: deleting.id },
      {
        onSuccess: () => {
          toast.success(t("accounts.toast.deleteSuccess"));
          setDeleting(null);
        },
      },
    );
  }, [deleteMut, deleting, t]);

  const handleValidate = useCallback(
    async (account: AccountView) => {
      setValidatingIds((prev) => {
        const next = new Set(prev);
        next.add(account.id);
        return next;
      });
      try {
        const outcome = await tauriInvoke<ValidationOutcome>("account_validate", {
          id: account.id,
        });
        if (outcome.valid) {
          toast.success(t("accounts.toast.validateSuccess"));
        } else {
          toast.error(
            t("accounts.toast.validateInvalid", {
              reason: outcome.errorMessage ?? "—",
            }),
          );
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        toast.error(`${t("accounts.toast.validateError")}: ${message}`);
      } finally {
        invalidateAccountsList();
        setValidatingIds((prev) => {
          const next = new Set(prev);
          next.delete(account.id);
          return next;
        });
      }
    },
    [t, invalidateAccountsList],
  );

  const rowActions = useMemo<AccountRowActions>(
    () => ({
      validate: handleValidate,
      edit: handleEdit,
      delete: requestDelete,
      toggleEnabled: handleToggleEnabled,
    }),
    [handleValidate, handleEdit, requestDelete, handleToggleEnabled],
  );

  const handleExport = useCallback(
    async (passphrase: string) => {
      const picked = await saveDialog({
        defaultPath: `vortex-accounts-${Date.now()}.vxbundle`,
        filters: [{ name: "Vortex bundle", extensions: ["vxbundle"] }],
      }).catch(() => null);
      if (!picked) {
        return;
      }
      try {
        const result = await tauriInvoke<ExportAccountsResult>("account_export", {
          path: picked,
          passphrase,
        });
        toast.success(t("accounts.toast.exportSuccess", { count: result.count }));
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        toast.error(`${t("accounts.toast.exportError")}: ${message}`);
        throw err;
      }
    },
    [t],
  );

  const pickImportFile = useCallback(async () => {
    const picked = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Vortex bundle", extensions: ["vxbundle"] }],
    }).catch(() => null);
    if (typeof picked === "string") return picked;
    return null;
  }, []);

  const handleImport = useCallback(
    async (path: string, passphrase: string) => {
      try {
        const result = await tauriInvoke<ImportAccountsResult>("account_import", {
          path,
          passphrase,
        });
        toast.success(t("accounts.toast.importSuccess", { count: result.imported }));
        if (result.skippedDuplicates > 0) {
          toast.success(t("accounts.toast.importSkipped", { count: result.skippedDuplicates }));
        }
        invalidateAccountsList();
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        toast.error(`${t("accounts.toast.importError")}: ${message}`);
        throw err;
      }
    },
    [t, invalidateAccountsList],
  );

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4" data-testid="accounts-view">
      <header className="flex items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">{t("accounts.title")}</h1>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            onClick={() => setImportOpen(true)}
            data-testid="accounts-import-trigger"
          >
            {t("accounts.actions.import")}
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => setExportOpen(true)}
            disabled={accounts.length === 0}
            data-testid="accounts-export-trigger"
          >
            {t("accounts.actions.export")}
          </Button>
          <Button type="button" onClick={() => setAddOpen(true)} data-testid="accounts-add-trigger">
            <Plus className="mr-1 h-4 w-4" aria-hidden />
            {t("accounts.actions.add")}
          </Button>
        </div>
      </header>

      <div
        className="flex flex-wrap items-center gap-2"
        role="tablist"
        aria-label={t("accounts.title")}
      >
        {FILTER_ORDER.map((value) => (
          <Button
            key={value}
            type="button"
            size="sm"
            variant={filter === value ? "default" : "outline"}
            onClick={() => setFilter(value)}
            role="tab"
            aria-selected={filter === value}
            data-testid={`accounts-filter-${value}`}
          >
            {t(`accounts.filter.${value}`)} ({counts[value]})
          </Button>
        ))}
      </div>

      {error && (
        <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          {error.message}
        </div>
      )}

      {isLoading ? (
        <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
          {t("accounts.loading")}
        </div>
      ) : (
        <AccountList
          accounts={filteredAccounts}
          actions={rowActions}
          validatingIds={validatingIds}
        />
      )}

      <AddAccountDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSubmit={handleAddSubmit}
        defaultType={filter === "all" ? "premium" : filter}
      />
      <DeleteAccountDialog
        account={deleting}
        onCancel={() => setDeleting(null)}
        onConfirm={confirmDelete}
        pending={deleteMut.isPending}
      />
      <EditAccountDialog
        account={editing}
        onCancel={() => setEditing(null)}
        onSubmit={handleEditSubmit}
      />
      <ExportAccountsDialog
        open={exportOpen}
        onOpenChange={setExportOpen}
        onSubmit={handleExport}
      />
      <ImportAccountsDialog
        open={importOpen}
        onOpenChange={setImportOpen}
        onPickFile={pickImportFile}
        onSubmit={handleImport}
      />
    </div>
  );
}
