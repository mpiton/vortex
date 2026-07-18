import { useTranslation } from "react-i18next";
import { Card, CardContent } from "@/components/ui/card";
import { CalendarClock } from "lucide-react";

// MAT-136 R-03: no REST/WS/Web UI server exists yet (planned for v0.4).
// The section only announces the feature so remote access can never look
// active. Restore the interactive controls when the server ships.
export function RemoteAccessSection() {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.remote.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.remote.description")}</p>
      </div>

      <Card>
        <CardContent className="flex items-start gap-3 pt-0">
          <CalendarClock className="mt-0.5 size-5 shrink-0 text-muted-foreground" />
          <p className="text-sm text-muted-foreground">{t("settings.remote.planned")}</p>
        </CardContent>
      </Card>
    </div>
  );
}
