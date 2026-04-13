import "@testing-library/jest-dom/vitest";
import { beforeEach, vi } from "vitest";

function getMockLanguage(): "en" | "fr" {
  if (typeof window === "undefined") return "en";
  return window.localStorage.getItem("i18nextLng") === "fr" ? "fr" : "en";
}

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

// Mock react-i18next globally. Tests default to English, but can switch to
// French by setting localStorage.i18nextLng before rendering.
vi.mock("react-i18next", async () => {
  const en = await import("./i18n/locales/en.json");
  const fr = await import("./i18n/locales/fr.json");

  const translations = {
    en: en.default as unknown as Record<string, unknown>,
    fr: fr.default as unknown as Record<string, unknown>,
  };

  const t = (key: string, options?: Record<string, unknown>) => {
    const template = lookupKey(translations[getMockLanguage()], key);

    if (!options) return template;

    return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => {
      const value = options[name];
      return value == null ? `{{${name}}}` : String(value);
    });
  };

  return {
    useTranslation: () => ({
      t,
      i18n: {
        language: getMockLanguage(),
        changeLanguage: vi.fn().mockImplementation(async (language: string) => {
          if (typeof window !== "undefined") {
            window.localStorage.setItem("i18nextLng", language);
          }
        }),
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

beforeEach(() => {
  window.localStorage.removeItem("i18nextLng");
});
