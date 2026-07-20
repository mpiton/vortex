export type CaptchaType = "image" | "text_input" | "recaptcha_v2" | "recaptcha_v3" | "hcaptcha";
export type CaptchaStatus = "pending" | "solved" | "skipped" | "timed_out";
export type CaptchaSolverAttemptOutcome =
  | "solved"
  | "unavailable"
  | "rejected"
  | "failed"
  | "interaction_required";

export interface CaptchaSolverAttempt {
  solver: string;
  outcome: CaptchaSolverAttemptOutcome;
  attemptedAt: number;
  durationMs: number;
}

export interface CaptchaChallengeView {
  id: string;
  downloadId: number;
  challengeType: CaptchaType;
  challengeUrl: string;
  imageData: string | null;
  imageMimeType: string | null;
  status: CaptchaStatus;
  solver: string | null;
  attempts: number;
  solverAttempts: CaptchaSolverAttempt[];
  createdAt: number;
  expiresAt: number;
  resolvedAt: number | null;
  durationMs: number | null;
  failureReason: string | null;
}
