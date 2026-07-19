import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { useSettingsStore } from "@/stores/settingsStore";

const DEFAULT_TIMEOUT_SECONDS = 120;

export function CaptchaSolverSettings() {
  const { t } = useTranslation();
  const configured = useSettingsStore(
    (state) => state.config?.captchaTimeoutSeconds ?? DEFAULT_TIMEOUT_SECONDS,
  );
  const updateConfig = useSettingsStore((state) => state.updateConfig);
  const [timeout, setTimeout] = useState(configured);

  useEffect(() => setTimeout(configured), [configured]);

  const persistTimeout = () => {
    const normalized = Math.min(3_600, Math.max(10, timeout || DEFAULT_TIMEOUT_SECONDS));
    setTimeout(normalized);
    void updateConfig({ captchaTimeoutSeconds: normalized });
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("captcha.settings.title")}</CardTitle>
        <CardDescription>{t("captcha.settings.description")}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div>
          <p className="text-sm font-medium">{t("captcha.settings.manual")}</p>
          <p className="text-sm text-muted-foreground">{t("captcha.settings.manualDescription")}</p>
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor="captcha-timeout">
            {t("captcha.settings.timeout")}
          </label>
          <Input
            id="captcha-timeout"
            max={3_600}
            min={10}
            onBlur={persistTimeout}
            onChange={(event) => setTimeout(Number(event.target.value))}
            type="number"
            value={timeout}
          />
          <p className="text-xs text-muted-foreground">{t("captcha.settings.fallback")}</p>
        </div>
      </CardContent>
    </Card>
  );
}
