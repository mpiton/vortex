import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch, ThemeMode } from '@/types/settings';
import { ACCENT_PRESETS } from '@/types/settings';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { SettingToggle } from './SettingField';
import { useLanguage, type Language } from '@/hooks/useLanguage';

interface AppearanceSectionProps {
  config: AppConfig;
}

const LOCALE_LABELS: Record<string, string> = {
  en: 'English',
  fr: 'Français',
};

const HEX_REGEX = /^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$/;

function normalizeHex(value: string): string {
  const trimmed = value.trim();
  if (/^#[0-9A-Fa-f]{3}$/.test(trimmed)) {
    const [, r, g, b] = trimmed.split('');
    return `#${r}${r}${g}${g}${b}${b}`.toUpperCase();
  }
  return trimmed.toUpperCase();
}

export function AppearanceSection({ config }: AppearanceSectionProps) {
  const { t } = useTranslation();
  const { setLanguage, availableLanguages } = useLanguage();
  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const [hexDraft, setHexDraft] = useState('');
  const [hexError, setHexError] = useState(false);

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ patch: { [key]: value } as AppConfigPatch });
  };

  const handleHexChange = (value: string) => {
    setHexDraft(value);
    setHexError(false);
  };

  const handleHexCommit = () => {
    if (hexDraft === '') return;
    if (!HEX_REGEX.test(hexDraft.trim())) {
      setHexError(true);
      return;
    }
    const normalized = normalizeHex(hexDraft);
    handleChange('accentColor', normalized);
    setHexDraft('');
    setHexError(false);
  };

  const isValidPreview = hexDraft !== '' && HEX_REGEX.test(hexDraft.trim());

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.appearance.title')}</h2>
        <p className="text-sm text-muted-foreground">{t('settings.appearance.description')}</p>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4 py-2">
          <p className="text-sm font-medium">{t('settings.appearance.theme')}</p>
          <Select
            value={config.theme}
            onValueChange={(v: ThemeMode) => handleChange('theme', v)}
          >
            <SelectTrigger className="w-32" aria-label={t('settings.appearance.theme')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="light">{t('settings.appearance.themeLight')}</SelectItem>
              <SelectItem value="dark">{t('settings.appearance.themeDark')}</SelectItem>
              <SelectItem value="auto">{t('settings.appearance.themeAuto')}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-2 py-2">
          <p className="text-sm font-medium">{t('settings.appearance.accentColor')}</p>
          <div className="flex gap-2">
            {ACCENT_PRESETS.map((preset) => (
              <button
                key={preset.value}
                type="button"
                aria-label={preset.name}
                aria-pressed={config.accentColor === preset.value}
                className={`size-8 rounded-full border-2 transition-transform hover:scale-110 ${
                  config.accentColor === preset.value
                    ? 'border-foreground scale-110'
                    : 'border-transparent'
                }`}
                style={{ backgroundColor: preset.value }}
                onClick={() => {
                  setHexError(false);
                  setHexDraft('');
                  handleChange('accentColor', preset.value);
                }}
              />
            ))}
          </div>
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Input
                value={hexDraft}
                onChange={(e) => handleHexChange(e.target.value)}
                onBlur={handleHexCommit}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleHexCommit();
                }}
                placeholder={t('settings.appearance.customHexColorPlaceholder')}
                aria-label={t('settings.appearance.customHexColor')}
                className={hexError ? 'border-destructive' : ''}
              />
              {hexError && (
                <p className="mt-1 text-xs text-destructive">
                  {t('settings.appearance.customHexColorInvalid')}
                </p>
              )}
            </div>
            <div
              className="size-8 shrink-0 rounded-full border-2 border-transparent transition-colors"
              style={{
                backgroundColor: isValidPreview
                  ? normalizeHex(hexDraft)
                  : config.accentColor,
              }}
              aria-label={t('settings.appearance.colorPreview')}
              title={t('settings.appearance.colorPreview')}
            />
          </div>
        </div>

        <SettingToggle
          label={t('settings.appearance.compactMode')}
          description={t('settings.appearance.compactModeDesc')}
          checked={config.compactMode}
          onCheckedChange={(v) => handleChange('compactMode', v)}
        />

        <div className="flex items-center justify-between gap-4 py-2">
          <p className="text-sm font-medium">{t('settings.appearance.language')}</p>
          <Select
            value={config.locale}
            onValueChange={(v) => {
              const lang = availableLanguages.includes(v as Language) ? (v as Language) : 'en';
              setLanguage(lang);
            }}
          >
            <SelectTrigger className="w-32" aria-label={t('settings.appearance.language')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {availableLanguages.map((lang) => (
                <SelectItem key={lang} value={lang}>
                  {LOCALE_LABELS[lang] ?? lang}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
    </div>
  );
}
