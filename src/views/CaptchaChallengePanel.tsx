import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { captchaQueries, downloadQueries } from "@/api/queries";
import { useTauriMutation } from "@/api/hooks";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { useCountdown } from "@/hooks/useCountdown";
import type { CaptchaChallengeView } from "@/types/captcha";

const INVALIDATE_KEYS = [captchaQueries.all(), downloadQueries.all()] as const;

export function CaptchaChallengePanel({ challenge }: { challenge: CaptchaChallengeView }) {
  const { t } = useTranslation();
  const [solution, setSolution] = useState("");
  const countdown = useCountdown(challenge.expiresAt);
  const acceptsText =
    challenge.challengeType === "image" || challenge.challengeType === "text_input";
  const totalMs = Math.max(1, challenge.expiresAt - challenge.createdAt);
  const progress = Math.min(100, (countdown.remainingSeconds * 1_000 * 100) / totalMs);

  useEffect(() => setSolution(""), [challenge.id]);

  const solve = useTauriMutation<void, { challengeId: string; solution: string }>("captcha_solve", {
    invalidateKeys: INVALIDATE_KEYS,
  });
  const skip = useTauriMutation<void, { challengeId: string }>("captcha_skip", {
    invalidateKeys: INVALIDATE_KEYS,
  });
  const retry = useTauriMutation<void, { challengeId: string }>("captcha_retry", {
    invalidateKeys: INVALIDATE_KEYS,
  });
  const busy = solve.isPending || skip.isPending || retry.isPending;

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("captcha.download", { id: challenge.downloadId })}</CardTitle>
        <div className="flex items-center justify-between text-sm text-muted-foreground">
          <span>{t(`captcha.types.${challenge.challengeType}`)}</span>
          <span data-testid="captcha-timer">{countdown.label}</span>
        </div>
        <Progress value={progress} />
      </CardHeader>
      <CardContent className="space-y-4">
        {challenge.imageData && challenge.imageMimeType ? (
          <CaptchaImage bytes={challenge.imageData} mimeType={challenge.imageMimeType} />
        ) : null}
        {acceptsText ? (
          <div className="space-y-2">
            <label className="text-sm font-medium" htmlFor="captcha-answer">
              {t("captcha.answer")}
            </label>
            <Input
              autoComplete="off"
              id="captcha-answer"
              maxLength={4_096}
              onChange={(event) => setSolution(event.target.value)}
              value={solution}
            />
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">{t("captcha.unsupported")}</p>
        )}
        <div className="flex flex-wrap gap-2">
          {acceptsText ? (
            <Button
              disabled={busy || solution.trim().length === 0}
              onClick={() => solve.mutate({ challengeId: challenge.id, solution })}
            >
              {t("captcha.actions.solve")}
            </Button>
          ) : null}
          <Button
            disabled={busy}
            onClick={() => skip.mutate({ challengeId: challenge.id })}
            variant="destructive"
          >
            {t("captcha.actions.skip")}
          </Button>
          <Button
            disabled={busy}
            onClick={() => retry.mutate({ challengeId: challenge.id })}
            variant="outline"
          >
            {t("captcha.actions.retry")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function CaptchaImage({ bytes, mimeType }: { bytes: number[]; mimeType: string }) {
  const [source, setSource] = useState<string | null>(null);

  useEffect(() => {
    const url = URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: mimeType }));
    setSource(url);
    return () => URL.revokeObjectURL(url);
  }, [bytes, mimeType]);

  return source ? (
    <img
      alt="CAPTCHA"
      className="mx-auto max-h-64 max-w-full rounded-md border object-contain"
      data-testid="captcha-image"
      src={source}
    />
  ) : null;
}
