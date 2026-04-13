import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

interface PlaceholderViewProps {
  icon: LucideIcon;
  titleKey: string;
}

export function PlaceholderView({ icon: Icon, titleKey }: PlaceholderViewProps) {
  const { t } = useTranslation();

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4">
      <Icon className="h-16 w-16 text-muted-foreground" />
      <div className="text-center">
        <h1 className="text-2xl font-bold">{t(titleKey)}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t("common.comingSoon")}</p>
      </div>
    </div>
  );
}
