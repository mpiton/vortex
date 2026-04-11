import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { SettingNumberInput } from './SettingField';

interface BrowserSectionProps {
  config: AppConfig;
}

export function BrowserSection({ config }: BrowserSectionProps) {
  const { mutate } = useTauriMutation<AppConfig, AppConfigPatch>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ [key]: value } as AppConfigPatch);
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Browser Integration</h2>
        <p className="text-sm text-muted-foreground">Browser extension capture settings</p>
      </div>

      <div className="space-y-4">
        <SettingNumberInput
          label="Minimum file size (MB)"
          description="Only capture files larger than this"
          value={config.minFileSizeMb}
          onChange={(v) => handleChange('minFileSizeMb', v)}
          min={0}
          step={0.1}
        />

        <div className="space-y-1">
          <p className="text-sm font-medium">Excluded domains</p>
          <p className="text-xs text-muted-foreground">Comma-separated list of domains to ignore</p>
          <textarea
            className="h-20 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            defaultValue={config.excludedDomains.join(', ')}
            onBlur={(e) => {
              const domains = e.target.value
                .split(',')
                .map((d) => d.trim())
                .filter(Boolean);
              handleChange('excludedDomains', domains);
            }}
          />
        </div>

        <div className="space-y-1">
          <p className="text-sm font-medium">Excluded extensions</p>
          <p className="text-xs text-muted-foreground">Comma-separated list of file extensions to ignore</p>
          <textarea
            className="h-20 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            defaultValue={config.excludedExtensions.join(', ')}
            onBlur={(e) => {
              const extensions = e.target.value
                .split(',')
                .map((ext) => ext.trim())
                .filter(Boolean);
              handleChange('excludedExtensions', extensions);
            }}
          />
        </div>
      </div>
    </div>
  );
}
