import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tauriInvoke } from "@/api/client";
import { useCaptchaQueue, usePendingCaptcha } from "@/hooks/useCaptchaQueue";
import { useTauriEvent } from "@/hooks/useTauriEvent";

vi.mock("@/api/client", () => ({ tauriInvoke: vi.fn() }));
vi.mock("@/hooks/useTauriEvent", () => ({ useTauriEvent: vi.fn() }));

const invoke = vi.mocked(tauriInvoke);

function wrapper(client: QueryClient) {
  return function QueryWrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

describe("useCaptchaQueue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invoke.mockResolvedValue([]);
  });

  it("subscribes to every CAPTCHA event and invalidates the queue", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries");

    renderHook(() => useCaptchaQueue(), { wrapper: wrapper(client) });

    expect(vi.mocked(useTauriEvent).mock.calls.map(([name]) => name)).toEqual([
      "captcha-pending",
      "captcha-solved",
      "captcha-skipped",
      "captcha-timed-out",
    ]);
    for (const [, callback] of vi.mocked(useTauriEvent).mock.calls) {
      act(() => callback({ challengeId: "captcha-1", downloadId: 42 }));
    }
    expect(invalidate).toHaveBeenCalledTimes(4);
    expect(invalidate).toHaveBeenLastCalledWith({ queryKey: ["captcha"] });
  });

  it("only fetches pending detail when an id is enabled", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { rerender } = renderHook(({ id }: { id?: string }) => usePendingCaptcha(id), {
      initialProps: { id: undefined } as { id?: string },
      wrapper: wrapper(client),
    });

    expect(invoke).not.toHaveBeenCalled();

    rerender({ id: "captcha-42" });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("captcha_get_pending", {
        challengeId: "captcha-42",
      }),
    );
  });
});
