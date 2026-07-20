import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { CaptchaBrowserWindow } from "../CaptchaBrowserWindow";

const closeWindow = vi.hoisted(() => vi.fn());
const eventListeners = vi.hoisted(
  () =>
    new Map<string, (event: { payload: { challengeId: string; downloadId: number } }) => void>(),
);

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: closeWindow }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (
      event: string,
      callback: (event: { payload: { challengeId: string; downloadId: number } }) => void,
    ) => {
      eventListeners.set(event, callback);
      return () => eventListeners.delete(event);
    },
  ),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  closeWindow.mockReset();
  closeWindow.mockResolvedValue(undefined);
  mockInvoke.mockReset();
  eventListeners.clear();
  mockInvoke.mockImplementation(async (command: string) => {
    if (command === "captcha_get_pending") {
      return {
        id: "captcha-1",
        downloadId: 42,
        challengeType: "image",
        challengeUrl: "https://hoster.example/file/42",
        imageData: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB",
        imageMimeType: "image/png",
        status: "pending",
        solver: null,
        attempts: 0,
        solverAttempts: [],
        createdAt: Date.now() - 1_000,
        expiresAt: Date.now() + 60_000,
        resolvedAt: null,
        durationMs: null,
        failureReason: null,
      };
    }
    return null;
  });
});

it("does not subscribe the isolated WebView to app-wide events", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <CaptchaBrowserWindow challengeId="captcha-1" />
    </QueryClientProvider>,
  );

  await screen.findByTestId("captcha-image");
  expect(eventListeners.size).toBe(0);
  expect(closeWindow).not.toHaveBeenCalled();
});

it("submits the human answer and closes the CAPTCHA WebView", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <CaptchaBrowserWindow challengeId="captcha-1" />
    </QueryClientProvider>,
  );

  const user = userEvent.setup();
  await user.type(await screen.findByLabelText("Captcha answer"), "human-answer");
  await user.click(screen.getByRole("button", { name: "Solve" }));

  await waitFor(() => expect(closeWindow).toHaveBeenCalledOnce());
  expect(mockInvoke).toHaveBeenCalledWith("captcha_solve", {
    challengeId: "captcha-1",
    solution: "human-answer",
  });
});
