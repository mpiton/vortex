import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Mock react-i18next globally — t(key) returns the English translation value
// so existing tests that assert on English text continue to work
vi.mock("react-i18next", async () => {
  const en = await import("./i18n/locales/en.json");

  function lookupKey(obj: Record<string, unknown>, key: string): string {
    const parts = key.split(".");
    let current: unknown = obj;
    for (const part of parts) {
      if (current && typeof current === "object") {
        current = (current as Record<string, unknown>)[part];
      } else {
        return key;
      }
    }
    return typeof current === "string" ? current : key;
  }

  const t = (key: string) => lookupKey(en.default as unknown as Record<string, unknown>, key);

  return {
    useTranslation: () => ({
      t,
      i18n: {
        language: "en",
        changeLanguage: vi.fn().mockResolvedValue(undefined),
      },
      ready: true,
    }),
    initReactI18next: { type: "3rdParty", init: vi.fn() },
    Trans: ({ children }: { children: unknown }) => children,
  };
});

// jsdom does not implement matchMedia — provide a minimal stub for all tests
Object.defineProperty(window, "matchMedia", {
  configurable: true,
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(() => true),
  })),
});
