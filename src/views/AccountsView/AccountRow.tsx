import { useTranslation } from "react-i18next";
import { MoreHorizontal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useLanguage } from "@/hooks/useLanguage";
import { formatBytes, formatDate } from "@/lib/format";
import type { AccountView } from "@/types/account";
import { deriveAccountStatus, type AccountStatus } from "./statusUtils";

export interface AccountRowActions {
  validate: (account: AccountView) => void;
  edit: (account: AccountView) => void;
  delete: (account: AccountView) => void;
  toggleEnabled: (account: AccountView, nextEnabled: boolean) => void;
}

interface AccountRowProps {
  account: AccountView;
  actions: AccountRowActions;
  validating?: boolean;
}

const STATUS_VARIANT: Record<AccountStatus, "default" | "secondary" | "destructive" | "outline"> = {
  active: "default",
  expired: "destructive",
  disabled: "secondary",
  unverified: "outline",
};

export function AccountRow({ account, actions, validating }: AccountRowProps) {
  const { t } = useTranslation();
  const { current: language } = useLanguage();
  const status = deriveAccountStatus(account);
  const trafficPercent = computeTrafficPercent(account.trafficLeft, account.trafficTotal);

  return (
    <tr data-testid={`account-row-${account.id}`} className="border-b last:border-b-0">
      <td className="px-3 py-2 align-middle font-medium">{account.serviceName}</td>
      <td className="px-3 py-2 align-middle text-sm text-muted-foreground">
        {account.username}
      </td>
      <td className="px-3 py-2 align-middle text-sm">
        {t(`accounts.filter.${account.accountType}`)}
      </td>
      <td className="px-3 py-2 align-middle">
        <Badge variant={STATUS_VARIANT[status]}>
          {t(`accounts.status.${status}`)}
        </Badge>
      </td>
      <td className="px-3 py-2 align-middle text-sm">
        {trafficPercent !== null ? (
          <div className="flex flex-col gap-1">
            <Progress
              value={trafficPercent}
              aria-label={t("accounts.traffic.ariaLabel")}
              className="h-2 w-32"
            />
            <span className="text-xs text-muted-foreground">
              {t("accounts.traffic.format", {
                used: formatBytes(Math.max(0, account.trafficTotal! - account.trafficLeft!)),
                total: formatBytes(account.trafficTotal),
              })}
            </span>
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">{t("accounts.traffic.unknown")}</span>
        )}
      </td>
      <td className="px-3 py-2 align-middle text-sm">
        {account.validUntil !== null
          ? formatDate(account.validUntil, language)
          : t("accounts.validUntil.never")}
      </td>
      <td className="px-3 py-2 align-middle text-sm text-muted-foreground">
        {account.lastValidated !== null
          ? formatDate(account.lastValidated, language)
          : "—"}
      </td>
      <td className="px-3 py-2 align-middle">
        <div className="flex items-center justify-end gap-2">
          <Switch
            checked={account.enabled}
            onCheckedChange={(checked) => actions.toggleEnabled(account, checked)}
            aria-label={t("accounts.status.active")}
          />
          <Button
            size="sm"
            variant="outline"
            onClick={() => actions.validate(account)}
            disabled={validating}
          >
            {t("accounts.actions.validate")}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("accounts.actions.more")}
                data-testid={`account-row-menu-${account.id}`}
              >
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => actions.edit(account)}>
                {t("accounts.actions.edit")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onSelect={() => actions.delete(account)}
                className="text-destructive focus:text-destructive"
              >
                {t("accounts.actions.delete")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </td>
    </tr>
  );
}

function computeTrafficPercent(
  trafficLeft: number | null,
  trafficTotal: number | null,
): number | null {
  if (trafficLeft === null || trafficTotal === null || trafficTotal <= 0) return null;
  const used = Math.max(0, trafficTotal - trafficLeft);
  return Math.min(100, (used / trafficTotal) * 100);
}
