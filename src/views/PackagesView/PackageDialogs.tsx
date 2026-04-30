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
import type { CreatePackageInput, PackageSourceType, PackageView } from "@/types/package";

const SOURCE_OPTIONS: PackageSourceType[] = ["manual", "playlist", "container", "split_archive"];

interface AddPackageDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: CreatePackageInput) => Promise<void>;
}

export function AddPackageDialog({ open, onOpenChange, onSubmit }: AddPackageDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [sourceType, setSourceType] = useState<PackageSourceType>("manual");
  const [folderPath, setFolderPath] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) {
      setName("");
      setSourceType("manual");
      setFolderPath("");
      setSubmitting(false);
    }
  }, [open]);

  const trimmedName = name.trim();
  const trimmedFolder = folderPath.trim();
  const canSubmit = !submitting && trimmedName.length > 0;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit({
        name: trimmedName,
        sourceType,
        folderPath: trimmedFolder.length > 0 ? trimmedFolder : undefined,
      });
      onOpenChange(false);
    } catch {
      // toast surfaced by mutation
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("packages.addDialog.title")}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.addDialog.name")}</span>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              autoFocus
              data-testid="package-add-name"
            />
          </label>
          <div className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.addDialog.sourceType")}</span>
            <Select value={sourceType} onValueChange={(v) => setSourceType(v as PackageSourceType)}>
              <SelectTrigger data-testid="package-add-source-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SOURCE_OPTIONS.map((opt) => (
                  <SelectItem key={opt} value={opt}>
                    {t(`packages.filter.${opt}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.addDialog.folderPath")}</span>
            <Input
              value={folderPath}
              onChange={(e) => setFolderPath(e.target.value)}
              data-testid="package-add-folder"
            />
          </label>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("packages.addDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="package-add-submit">
              {t("packages.addDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface RenamePackageDialogProps {
  pkg: PackageView | null;
  onCancel: () => void;
  onSubmit: (name: string) => Promise<void>;
}

export function RenamePackageDialog({ pkg, onCancel, onSubmit }: RenamePackageDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const open = pkg !== null;
  const initialNameRef = useRef("");
  initialNameRef.current = pkg?.name ?? "";

  useEffect(() => {
    if (open) {
      setName(initialNameRef.current);
      setSubmitting(false);
    }
  }, [open]);

  const trimmed = name.trim();
  const canSubmit =
    !submitting && trimmed.length > 0 && trimmed !== initialNameRef.current.trim();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit(trimmed);
      onCancel();
    } catch {
      // toast surfaced by mutation
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("packages.renameDialog.title")}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.renameDialog.name")}</span>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              autoFocus
              data-testid="package-rename-input"
            />
          </label>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onCancel} disabled={submitting}>
              {t("packages.renameDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="package-rename-submit">
              {t("packages.renameDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface PasswordDialogProps {
  pkg: PackageView | null;
  onCancel: () => void;
  onSubmit: (password: string | null) => Promise<void>;
}

export function PasswordDialog({ pkg, onCancel, onSubmit }: PasswordDialogProps) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const open = pkg !== null;

  useEffect(() => {
    if (open) {
      setPassword("");
      setSubmitting(false);
    }
  }, [open]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (submitting) return;
    setSubmitting(true);
    try {
      await onSubmit(password.length > 0 ? password : null);
      onCancel();
    } catch {
      // toast surfaced by mutation
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("packages.passwordDialog.title")}</DialogTitle>
          <DialogDescription>{t("packages.passwordDialog.description")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.passwordDialog.password")}</span>
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("packages.passwordDialog.passwordPlaceholder")}
              autoFocus
              data-testid="package-password-input"
            />
          </label>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onCancel} disabled={submitting}>
              {t("packages.passwordDialog.cancel")}
            </Button>
            <Button type="submit" disabled={submitting} data-testid="package-password-submit">
              {t("packages.passwordDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface FolderDialogProps {
  pkg: PackageView | null;
  onCancel: () => void;
  onPickFolder: () => Promise<string | null>;
  onSubmit: (folder: string) => Promise<void>;
}

export function FolderDialog({ pkg, onCancel, onPickFolder, onSubmit }: FolderDialogProps) {
  const { t } = useTranslation();
  const [folder, setFolder] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const open = pkg !== null;
  const initialFolderRef = useRef<string>("");
  initialFolderRef.current = pkg?.folderPath ?? "";

  useEffect(() => {
    if (open) {
      setFolder(initialFolderRef.current);
      setSubmitting(false);
    }
  }, [open]);

  const trimmed = folder.trim();
  const canSubmit =
    !submitting && trimmed.length > 0 && trimmed !== initialFolderRef.current.trim();

  const handleBrowse = async () => {
    const picked = await onPickFolder();
    if (picked) setFolder(picked);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit(trimmed);
      onCancel();
    } catch {
      // toast surfaced by mutation
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("packages.folderDialog.title")}</DialogTitle>
          <DialogDescription>{t("packages.folderDialog.description")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <label className="grid gap-1 text-sm">
            <span className="font-medium">{t("packages.folderDialog.folder")}</span>
            <div className="flex gap-2">
              <Input
                value={folder}
                onChange={(e) => setFolder(e.target.value)}
                required
                data-testid="package-folder-input"
              />
              <Button
                type="button"
                variant="outline"
                onClick={handleBrowse}
                data-testid="package-folder-browse"
              >
                {t("packages.folderDialog.browse")}
              </Button>
            </div>
          </label>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onCancel} disabled={submitting}>
              {t("packages.folderDialog.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="package-folder-submit">
              {t("packages.folderDialog.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface DeletePackageDialogProps {
  pkg: PackageView | null;
  onCancel: () => void;
  onConfirm: (deleteDownloads: boolean) => Promise<void>;
}

export function DeletePackageDialog({ pkg, onCancel, onConfirm }: DeletePackageDialogProps) {
  const { t } = useTranslation();
  const [deleteDownloads, setDeleteDownloads] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const open = pkg !== null;

  useEffect(() => {
    if (open) {
      setDeleteDownloads(false);
      setSubmitting(false);
    }
  }, [open]);

  const handleConfirm = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await onConfirm(deleteDownloads);
      onCancel();
    } catch {
      // toast surfaced by mutation
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("packages.deleteDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("packages.deleteDialog.description", { name: pkg?.name ?? "" })}
          </DialogDescription>
        </DialogHeader>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={deleteDownloads}
            onChange={(e) => setDeleteDownloads(e.target.checked)}
            data-testid="package-delete-also-downloads"
          />
          {t("packages.deleteDialog.deleteDownloads")}
        </label>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onCancel} disabled={submitting}>
            {t("packages.deleteDialog.cancel")}
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={handleConfirm}
            disabled={submitting}
            data-testid="package-delete-confirm"
          >
            {t("packages.deleteDialog.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
