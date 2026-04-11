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
import { SettingToggle } from './SettingField';

interface AppearanceSectionProps {
  config: AppConfig;
}

const LOCALES = [
  { value: 'en', label: 'English' },
  { value: 'fr', label: 'Francais' },
  { value: 'de', label: 'Deutsch' },
  { value: 'es', label: 'Espanol' },
  { value: 'ja', label: 'Japanese' },
  { value: 'zh', label: 'Chinese' },
] as const;

export function AppearanceSection({ config }: AppearanceSectionProps) {
  const { mutate } = useTauriMutation<AppConfig, AppConfigPatch>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ [key]: value } as AppConfigPatch);
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Appearance</h2>
        <p className="text-sm text-muted-foreground">Theme and display preferences</p>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4 py-2">
          <p className="text-sm font-medium">Theme</p>
          <Select
            value={config.theme}
            onValueChange={(v: ThemeMode) => handleChange('theme', v)}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="light">Light</SelectItem>
              <SelectItem value="dark">Dark</SelectItem>
              <SelectItem value="auto">Auto</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-2 py-2">
          <p className="text-sm font-medium">Accent color</p>
          <div className="flex gap-2">
            {ACCENT_PRESETS.map((preset) => (
              <button
                key={preset.value}
                type="button"
                aria-label={preset.name}
                className={`size-8 rounded-full border-2 transition-transform hover:scale-110 ${
                  config.accentColor === preset.value
                    ? 'border-foreground scale-110'
                    : 'border-transparent'
                }`}
                style={{ backgroundColor: preset.value }}
                onClick={() => handleChange('accentColor', preset.value)}
              />
            ))}
          </div>
        </div>

        <SettingToggle
          label="Compact mode"
          description="Reduce spacing and font sizes"
          checked={config.compactMode}
          onCheckedChange={(v) => handleChange('compactMode', v)}
        />

        <div className="flex items-center justify-between gap-4 py-2">
          <p className="text-sm font-medium">Language</p>
          <Select
            value={config.locale}
            onValueChange={(v) => handleChange('locale', v)}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LOCALES.map((loc) => (
                <SelectItem key={loc.value} value={loc.value}>
                  {loc.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
    </div>
  );
}
