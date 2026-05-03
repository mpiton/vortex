import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/settingsStore";

// MVP: Only en/fr have translation files. Backend also accepts de, es, ja, zh.
// Expand this list and add locale JSON files when adding new translations.
const AVAILABLE_LANGUAGES = ["en", "fr"] as const;
export type Language = (typeof AVAILABLE_LANGUAGES)[number];

interface UseLanguageReturn {
  current: Language;
  setLanguage: (lang: Language) => void;
  availableLanguages: readonly Language[];
}

export function useLanguage(): UseLanguageReturn {
  const { i18n } = useTranslation();
  const updateConfig = useSettingsStore((s) => s.updateConfig);

  const setLanguage = (lang: Language) => {
    i18n.changeLanguage(lang);
    updateConfig({ locale: lang });
  };

  const base = i18n.language.split("-")[0] || i18n.language;
  const current: Language = (AVAILABLE_LANGUAGES as readonly string[]).includes(base)
    ? (base as Language)
    : "en";

  return {
    current,
    setLanguage,
    availableLanguages: AVAILABLE_LANGUAGES,
  };
}
