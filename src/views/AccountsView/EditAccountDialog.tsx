import { useEffect, useState } from "react";
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
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AccountPatch, AccountType, AccountView } from "@/types/account";

interface EditAccountDialogProps {
  account: AccountView | null;
  onCancel: () => void;
  onSubmit: (patch: AccountPatch) => Promise<void>;
}

const TYPE_OPTIONS: AccountType[] = ["debrid", "premium", "free"];

export function EditAccountDialog({ account, onCancel, onSubmit }: EditAccountDialogProps) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [accountType, setAccountType] = useState<AccountType>("premium");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (account) {
      setUsername(account.username);
      setPassword("");
      setAccountType(account.accountType);
      setSubmitting(false);
    }
  }, [account]);

  const trimmedUsername = username.trim();
  const canSubmit = !submitting && account !== null && trimmedUsername.length > 0;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit || account === null) return;
    const patch: AccountPatch = {};
    if (trimmedUsername !== account.username) patch.username = trimmedUsername;
    if (password.length > 0) patch.password = password;
    if (accountType !== account.accountType) patch.accountType = accountType;
    if (Object.keys(patch).length === 0) {
      onCancel();
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(patch);
      onCancel();
    } catch {
      // Toast surfaced by mutation; keep dialog open.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog
      open={account !== null}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("accounts.actions.edit")}</DialogTitle>
          <DialogDescription>
            {account?.serviceName ?? ""}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.username")}</span>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              data-testid="account-edit-username"
            />
          </label>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.password")}</span>
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••"
              data-testid="account-edit-password"
            />
          </label>
          <div className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.type")}</span>
            <Select
              value={accountType}
              onValueChange={(v) => setAccountType(v as AccountType)}
            >
              <SelectTrigger data-testid="account-edit-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TYPE_OPTIONS.map((opt) => (
                  <SelectItem key={opt} value={opt}>
                    {t(`accounts.filter.${opt}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onCancel} disabled={submitting}>
              {t("accounts.addDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="account-edit-submit">
              {t("accounts.addDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
