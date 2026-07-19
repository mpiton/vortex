export type CaptchaType = "image" | "text_input" | "recaptcha_v2" | "recaptcha_v3" | "hcaptcha";
export type CaptchaStatus = "pending" | "solved" | "skipped" | "timed_out";

export interface CaptchaChallengeView {
  id: string;
  downloadId: number;
  challengeType: CaptchaType;
  challengeUrl: string;
  imageData: number[] | null;
  imageMimeType: string | null;
  status: CaptchaStatus;
  solver: string | null;
  attempts: number;
  createdAt: number;
  expiresAt: number;
  resolvedAt: number | null;
  durationMs: number | null;
  failureReason: string | null;
}
