import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { AccountView } from "@/types/account";

interface DeleteAccountDialogProps {
  account: AccountView | null;
  onCancel: () => void;
  onConfirm: () => void;
  pending: boolean;
}

export function DeleteAccountDialog({
  account,
  onCancel,
  onConfirm,
  pending,
}: DeleteAccountDialogProps) {
  const { t } = useTranslation();
  const open = account !== null;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("accounts.deleteDialog.title")}</DialogTitle>
          <DialogDescription>
            {account &&
              t("accounts.deleteDialog.description", {
                username: account.username,
                service: account.serviceName,
              })}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onCancel} disabled={pending}>
            {t("accounts.deleteDialog.cancel")}
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={onConfirm}
            disabled={pending}
            data-testid="account-delete-confirm"
          >
            {t("accounts.deleteDialog.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
