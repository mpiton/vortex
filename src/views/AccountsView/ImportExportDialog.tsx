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

interface ExportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (passphrase: string) => Promise<void>;
}

export function ExportAccountsDialog({ open, onOpenChange, onSubmit }: ExportDialogProps) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const [confirmPassphrase, setConfirmPassphrase] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) {
      setPassphrase("");
      setConfirmPassphrase("");
      setSubmitting(false);
    }
  }, [open]);

  const mismatch = confirmPassphrase.length > 0 && passphrase !== confirmPassphrase;
  const canSubmit =
    !submitting && passphrase.length > 0 && passphrase === confirmPassphrase;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit(passphrase);
      onOpenChange(false);
    } catch {
      // Toast surfaced by caller; keep dialog open.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("accounts.exportDialog.title")}</DialogTitle>
          <DialogDescription>{t("accounts.exportDialog.description")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.exportDialog.passphrase")}</span>
            <Input
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              required
              autoFocus
              data-testid="account-export-passphrase"
            />
          </label>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.exportDialog.passphraseConfirm")}</span>
            <Input
              type="password"
              value={confirmPassphrase}
              onChange={(e) => setConfirmPassphrase(e.target.value)}
              required
              data-testid="account-export-passphrase-confirm"
              aria-invalid={mismatch ? true : undefined}
            />
            {mismatch && (
              <span className="text-xs text-destructive">
                {t("accounts.exportDialog.passphraseMismatch")}
              </span>
            )}
          </label>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("accounts.exportDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="account-export-submit">
              {t("accounts.exportDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPickFile: () => Promise<string | null>;
  onSubmit: (path: string, passphrase: string) => Promise<void>;
}

export function ImportAccountsDialog({
  open,
  onOpenChange,
  onPickFile,
  onSubmit,
}: ImportDialogProps) {
  const { t } = useTranslation();
  const [path, setPath] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) {
      setPath(null);
      setPassphrase("");
      setSubmitting(false);
    }
  }, [open]);

  const handleBrowse = async () => {
    const picked = await onPickFile();
    if (picked) setPath(picked);
  };

  const canSubmit = !submitting && path !== null && passphrase.length > 0;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit || path === null) return;
    setSubmitting(true);
    try {
      await onSubmit(path, passphrase);
      onOpenChange(false);
    } catch {
      // Toast surfaced by caller; keep dialog open.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("accounts.importDialog.title")}</DialogTitle>
          <DialogDescription>{t("accounts.importDialog.description")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <div className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.importDialog.filePath")}</span>
            <div className="flex items-center gap-2">
              <Input
                readOnly
                value={path ?? ""}
                placeholder="—"
                aria-label={t("accounts.importDialog.filePath")}
                data-testid="account-import-path"
              />
              <Button type="button" variant="outline" onClick={handleBrowse}>
                {t("accounts.importDialog.browse")}
              </Button>
            </div>
          </div>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("accounts.importDialog.passphrase")}</span>
            <Input
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              required
              data-testid="account-import-passphrase"
            />
          </label>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("accounts.importDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="account-import-submit">
              {t("accounts.importDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
