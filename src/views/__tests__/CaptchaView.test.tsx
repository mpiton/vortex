import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CaptchaView } from "../CaptchaView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(vi.fn()) }));

const mockInvoke = vi.mocked(invoke);

const pendingCaptcha = {
  id: "captcha-1",
  downloadId: 42,
  challengeType: "image",
  challengeUrl: "https://hoster.example/file/42",
  imageData: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB",
  imageMimeType: "image/png",
  status: "pending",
  solver: null,
  attempts: 0,
  createdAt: Date.now() - 1_000,
  expiresAt: Date.now() + 60_000,
  resolvedAt: null,
  durationMs: null,
  failureReason: null,
};

function renderView() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <CaptchaView />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(async (command: string) => {
    if (command === "captcha_list") {
      return [{ ...pendingCaptcha, imageData: null, imageMimeType: null }];
    }
    if (command === "captcha_get_pending") return pendingCaptcha;
    return null;
  });
});

describe("CaptchaView", () => {
  it("renders the pending image, input, timer and manual solver", async () => {
    renderView();

    expect(await screen.findByTestId("captcha-image")).toHaveAttribute(
      "src",
      `data:image/png;base64,${pendingCaptcha.imageData}`,
    );
    expect(screen.getByLabelText("Captcha answer")).toBeInTheDocument();
    expect(screen.getByTestId("captcha-timer")).toHaveTextContent(/\d{2}:\d{2}/);
    expect(screen.getByText("Manual solver")).toBeInTheDocument();
  });

  it("submits the answer through captcha_solve", async () => {
    renderView();
    const user = userEvent.setup();

    await user.type(await screen.findByLabelText("Captcha answer"), "abc123");
    await user.click(screen.getByRole("button", { name: "Solve" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("captcha_solve", {
        challengeId: "captcha-1",
        solution: "abc123",
      }),
    );
  });

  it("exposes skip and retry actions", async () => {
    renderView();
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "Skip" }));
    await user.click(screen.getByRole("button", { name: "Retry" }));

    expect(mockInvoke).toHaveBeenCalledWith("captcha_skip", { challengeId: "captcha-1" });
    expect(mockInvoke).toHaveBeenCalledWith("captcha_retry", { challengeId: "captcha-1" });
  });

  it("renders an empty queue", async () => {
    mockInvoke.mockResolvedValue([]);
    renderView();

    expect(await screen.findByText("No CAPTCHA waiting")).toBeInTheDocument();
  });

  it("selects the oldest pending challenge and marks the active row", async () => {
    const newest = { ...pendingCaptcha, id: "captcha-newest", downloadId: 43, createdAt: 2_000 };
    const oldest = { ...pendingCaptcha, id: "captcha-oldest", downloadId: 41, createdAt: 1_000 };
    mockInvoke.mockImplementation(async (command: string, args) => {
      if (command === "captcha_list") {
        return [newest, oldest].map((challenge) => ({
          ...challenge,
          imageData: null,
          imageMimeType: null,
        }));
      }
      if (command === "captcha_get_pending") {
        return (args as { challengeId?: string } | undefined)?.challengeId === oldest.id
          ? oldest
          : newest;
      }
      return null;
    });
    renderView();

    const oldestRow = await screen.findByRole("button", { name: /Download #41/ });
    const newestRow = screen.getByRole("button", { name: /Download #43/ });
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("captcha_get_pending", {
        challengeId: "captcha-oldest",
      }),
    );
    expect(oldestRow).toHaveAttribute("aria-pressed", "true");
    expect(newestRow).toHaveAttribute("aria-pressed", "false");

    await userEvent.click(newestRow);

    expect(newestRow).toHaveAttribute("aria-pressed", "true");
  });
});
