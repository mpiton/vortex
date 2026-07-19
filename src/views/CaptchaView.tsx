import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useCaptchaQueue, usePendingCaptcha } from "@/hooks/useCaptchaQueue";
import { CaptchaChallengePanel } from "./CaptchaChallengePanel";
import { CaptchaSolverSettings } from "./CaptchaSolverSettings";

export function CaptchaView() {
  const { t } = useTranslation();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const { data = [], isLoading, error } = useCaptchaQueue();
  const pending = data.filter((challenge) => challenge.status === "pending");
  const history = data.filter((challenge) => challenge.status !== "pending");
  const selected = pending.find((challenge) => challenge.id === selectedId) ?? pending[0];
  const { data: selectedDetail } = usePendingCaptcha(selected?.id);

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 p-4" data-testid="captcha-view">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t("captcha.title")}</h1>
        <Badge variant="secondary">{t("captcha.pendingCount", { count: pending.length })}</Badge>
      </header>

      {isLoading ? <p className="text-sm text-muted-foreground">{t("captcha.loading")}</p> : null}
      {error ? <p className="text-sm text-destructive">{t("captcha.error")}</p> : null}

      {!isLoading && !error ? (
        <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
          <Card className="min-h-0 gap-3 py-4">
            <CardHeader className="px-4">
              <CardTitle>{t("captcha.queue")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 overflow-auto px-4">
              {pending.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("captcha.empty")}</p>
              ) : (
                pending.map((challenge) => (
                  <button
                    className="w-full rounded-md border p-3 text-left hover:bg-accent"
                    key={challenge.id}
                    onClick={() => setSelectedId(challenge.id)}
                    type="button"
                  >
                    <span className="block text-sm font-medium">
                      {t("captcha.download", { id: challenge.downloadId })}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {t(`captcha.types.${challenge.challengeType}`)}
                    </span>
                  </button>
                ))
              )}
            </CardContent>
          </Card>

          <div className="min-h-0 space-y-4 overflow-auto">
            {selected ? <CaptchaChallengePanel challenge={selectedDetail ?? selected} /> : null}
            <CaptchaSolverSettings />
            {history.length > 0 ? (
              <Card className="gap-3 py-4">
                <CardHeader className="px-4">
                  <CardTitle>{t("captcha.history")}</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 px-4">
                  {history.slice(0, 10).map((challenge) => (
                    <div className="flex items-center justify-between text-sm" key={challenge.id}>
                      <span>{t("captcha.download", { id: challenge.downloadId })}</span>
                      <Badge variant="outline">{t(`captcha.status.${challenge.status}`)}</Badge>
                    </div>
                  ))}
                </CardContent>
              </Card>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
