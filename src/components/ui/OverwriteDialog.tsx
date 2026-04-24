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

export type OverwriteDecision = "overwrite" | "rename" | "cancel";

interface OverwriteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  originalPath: string;
  suggestedPath: string;
  onDecision: (decision: OverwriteDecision) => void;
}

export function OverwriteDialog({
  open,
  onOpenChange,
  originalPath,
  suggestedPath,
  onDecision,
}: OverwriteDialogProps) {
  const { t } = useTranslation();

  const handle = (decision: OverwriteDecision) => {
    onDecision(decision);
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onDecision("cancel");
        }
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("common.overwriteDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("common.overwriteDialog.description", { path: originalPath })}
          </DialogDescription>
        </DialogHeader>

        <div className="text-sm">
          <span className="font-medium">{t("common.overwriteDialog.suggestedPathLabel")}</span>
          <p
            className="mt-1 break-all rounded-md bg-muted/50 p-2 font-mono text-xs"
            data-testid="overwrite-suggested-path"
          >
            {suggestedPath}
          </p>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => handle("cancel")}>
            {t("common.overwriteDialog.cancel")}
          </Button>
          <Button variant="secondary" onClick={() => handle("rename")}>
            {t("common.overwriteDialog.rename")}
          </Button>
          <Button variant="destructive" onClick={() => handle("overwrite")}>
            {t("common.overwriteDialog.overwrite")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
