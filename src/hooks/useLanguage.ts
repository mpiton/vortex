import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';

const AVAILABLE_LANGUAGES = ['en', 'fr'] as const;
export type Language = (typeof AVAILABLE_LANGUAGES)[number];

interface UseLanguageReturn {
  current: string;
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

  return {
    current: i18n.language,
    setLanguage,
    availableLanguages: AVAILABLE_LANGUAGES,
  };
}
