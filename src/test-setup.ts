import "@testing-library/jest-dom/vitest";
import { beforeEach, vi } from "vitest";

function resolveMockLanguage(options?: Record<string, unknown>): "en" | "fr" {
  const requestedLanguage = options?.lng;
  if (requestedLanguage === "fr") return "fr";
  if (requestedLanguage === "en") return "en";
  return getMockLanguage();
}

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
    const language = resolveMockLanguage(options);
    const count = options?.count;
    const pluralCategory =
      typeof count === "number" ? new Intl.PluralRules(language).select(count) : null;
    const pluralKey = pluralCategory === null ? key : `${key}_${pluralCategory}`;
    const pluralTemplate = lookupKey(translations[language], pluralKey);
    const template =
      pluralTemplate !== pluralKey ? pluralTemplate : lookupKey(translations[language], key);

    if (!options) return template;

    return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => {
      const value = options[name];
      return value === null || value === undefined ? `{{${name}}}` : String(value);
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

// Mock sonner globally so tests don't render actual toasts and can assert
// toast.error / toast.success calls when needed. The surface mirrors
// `src/lib/toast.ts` (the only public channel to sonner in production).
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
  Toaster: () => null,
}));

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
  vi.clearAllMocks();
});
