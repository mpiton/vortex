import { useTranslation } from "react-i18next";
import type { AccountView } from "@/types/account";
import { AccountRow, type AccountRowActions } from "./AccountRow";

interface AccountListProps {
  accounts: AccountView[];
  actions: AccountRowActions;
  validatingId: string | null;
}

export function AccountList({ accounts, actions, validatingId }: AccountListProps) {
  const { t } = useTranslation();

  if (accounts.length === 0) {
    return (
      <div
        data-testid="accounts-empty"
        className="flex h-32 items-center justify-center text-sm text-muted-foreground"
      >
        {t("accounts.empty")}
      </div>
    );
  }

  return (
    <div className="overflow-auto rounded-md border">
      <table className="w-full text-left text-sm">
        <thead className="bg-muted/50 text-xs uppercase tracking-wide text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.service")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.username")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.type")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.status")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.traffic")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.validUntil")}</th>
            <th className="px-3 py-2 font-medium">{t("accounts.columns.lastValidated")}</th>
            <th className="px-3 py-2 text-right font-medium">{t("accounts.columns.actions")}</th>
          </tr>
        </thead>
        <tbody>
          {accounts.map((account) => (
            <AccountRow
              key={account.id}
              account={account}
              actions={actions}
              validating={validatingId === account.id}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}
