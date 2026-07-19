import { useQuery, useQueryClient } from "@tanstack/react-query";
import { tauriInvoke } from "@/api/client";
import { captchaQueries } from "@/api/queries";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { CaptchaChallengeView } from "@/types/captcha";
import type { CaptchaEventPayload } from "@/types/events";

const CAPTCHA_EVENTS = [
  "captcha-pending",
  "captcha-solved",
  "captcha-skipped",
  "captcha-timed-out",
] as const;

export function useCaptchaQueue() {
  const queryClient = useQueryClient();
  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: captchaQueries.all() });
  };

  useTauriEvent<CaptchaEventPayload>(CAPTCHA_EVENTS[0], invalidate);
  useTauriEvent<CaptchaEventPayload>(CAPTCHA_EVENTS[1], invalidate);
  useTauriEvent<CaptchaEventPayload>(CAPTCHA_EVENTS[2], invalidate);
  useTauriEvent<CaptchaEventPayload>(CAPTCHA_EVENTS[3], invalidate);

  return useQuery<CaptchaChallengeView[], Error>({
    queryKey: captchaQueries.list(),
    queryFn: () => tauriInvoke<CaptchaChallengeView[]>("captcha_list"),
  });
}

export function usePendingCaptcha(challengeId: string | undefined) {
  return useQuery<CaptchaChallengeView | null, Error>({
    enabled: challengeId !== undefined,
    queryKey: captchaQueries.detail(challengeId ?? ""),
    queryFn: () => tauriInvoke<CaptchaChallengeView | null>("captcha_get_pending", { challengeId }),
  });
}
