import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { usePendingCaptcha } from "@/hooks/useCaptchaQueue";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { CaptchaEventPayload } from "@/types/events";
import { CaptchaChallengePanel } from "./CaptchaChallengePanel";

export function CaptchaBrowserWindow({ challengeId }: { challengeId: string }) {
  const { t } = useTranslation();
  const { data: challenge, isLoading, error } = usePendingCaptcha(challengeId);
  const close = () => {
    void getCurrentWindow().close();
  };
  const closeIfResolved = (payload: CaptchaEventPayload) => {
    if (payload.challengeId === challengeId) close();
  };

  useTauriEvent<CaptchaEventPayload>("captcha-solved", closeIfResolved);
  useTauriEvent<CaptchaEventPayload>("captcha-skipped", closeIfResolved);
  useTauriEvent<CaptchaEventPayload>("captcha-timed-out", closeIfResolved);

  return (
    <main className="min-h-screen bg-background p-4 text-foreground">
      <h1 className="mb-4 text-xl font-semibold">{t("captcha.browserTitle")}</h1>
      {isLoading ? <p>{t("captcha.loading")}</p> : null}
      {error ? <p className="text-destructive">{t("captcha.error")}</p> : null}
      {!isLoading && !error && !challenge ? <p>{t("captcha.browserUnavailable")}</p> : null}
      {challenge ? <CaptchaChallengePanel challenge={challenge} onResolved={close} /> : null}
    </main>
  );
}
