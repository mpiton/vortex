import { useEffect, useRef, useState } from "react";
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
import type { AccountType, AddAccountInput } from "@/types/account";

interface AddAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defaultType?: AccountType;
  onSubmit: (input: AddAccountInput) => Promise<void>;
}

const TYPE_OPTIONS: AccountType[] = ["debrid", "premium", "free"];

export function AddAccountDialog({
  open,
  onOpenChange,
  defaultType = "premium",
  onSubmit,
}: AddAccountDialogProps) {
  const { t } = useTranslation();
  const [serviceName, setServiceName] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [accountType, setAccountType] = useState<AccountType>(defaultType);
  const [submitting, setSubmitting] = useState(false);

  const defaultTypeRef = useRef(defaultType);
  defaultTypeRef.current = defaultType;

  useEffect(() => {
    if (open) {
      setServiceName("");
      setUsername("");
      setPassword("");
      setAccountType(defaultTypeRef.current);
      setSubmitting(false);
    }
  }, [open]);

  const trimmedService = serviceName.trim();
  const trimmedUsername = username.trim();
  const canSubmit =
    !submitting &&
    trimmedService.length > 0 &&
    trimmedUsername.length > 0 &&
    password.length > 0;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit({
        serviceName: trimmedService,
        username: trimmedUsername,
        password,
        accountType,
      });
      onOpenChange(false);
    } catch {
      // Error toast surfaced by mutation; keep dialog open for retry.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("accounts.addDialog.title")}</DialogTitle>
          <DialogDescription>{t("accounts.addDialog.description")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.service")}</span>
            <Input
              value={serviceName}
              placeholder={t("accounts.addDialog.servicePlaceholder")}
              onChange={(e) => setServiceName(e.target.value)}
              autoFocus
              required
              data-testid="account-add-service"
            />
          </label>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.username")}</span>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              data-testid="account-add-username"
            />
          </label>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.password")}</span>
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              data-testid="account-add-password"
            />
          </label>
          <div className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.addDialog.type")}</span>
            <Select
              value={accountType}
              onValueChange={(v) => setAccountType(v as AccountType)}
            >
              <SelectTrigger data-testid="account-add-type">
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
            <span className="text-xs text-muted-foreground">
              {t("accounts.addDialog.typeHint")}
            </span>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("accounts.addDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="account-add-submit">
              {t("accounts.addDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
