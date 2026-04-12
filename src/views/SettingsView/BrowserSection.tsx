import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { SettingNumberInput } from './SettingField';

interface BrowserSectionProps {
  config: AppConfig;
}

export function BrowserSection({ config }: BrowserSectionProps) {
  const { t } = useTranslation();
  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ patch: { [key]: value } as AppConfigPatch });
  };

  const [domainsDraft, setDomainsDraft] = useState(config.excludedDomains.join(', '));
  const [extensionsDraft, setExtensionsDraft] = useState(config.excludedExtensions.join(', '));

  useEffect(() => { setDomainsDraft(config.excludedDomains.join(', ')); }, [config.excludedDomains]);
  useEffect(() => { setExtensionsDraft(config.excludedExtensions.join(', ')); }, [config.excludedExtensions]);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.browser.title')}</h2>
        <p className="text-sm text-muted-foreground">{t('settings.browser.description')}</p>
      </div>

      <div className="space-y-4">
        <SettingNumberInput
          label={t('settings.browser.minFileSize')}
          description={t('settings.browser.minFileSizeDesc')}
          value={config.minFileSizeMb}
          onChange={(v) => handleChange('minFileSizeMb', v)}
          min={0}
          step={0.1}
        />

        <div className="space-y-1">
          <p className="text-sm font-medium">{t('settings.browser.excludedDomains')}</p>
          <p className="text-xs text-muted-foreground">{t('settings.browser.excludedDomainsDesc')}</p>
          <textarea
            className="h-20 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            value={domainsDraft}
            onChange={(e) => setDomainsDraft(e.target.value)}
            onBlur={() => {
              const domains = domainsDraft
                .split(',')
                .map((d) => d.trim())
                .filter(Boolean);
              handleChange('excludedDomains', domains);
            }}
          />
        </div>

        <div className="space-y-1">
          <p className="text-sm font-medium">{t('settings.browser.excludedExtensions')}</p>
          <p className="text-xs text-muted-foreground">{t('settings.browser.excludedExtensionsDesc')}</p>
          <textarea
            className="h-20 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            value={extensionsDraft}
            onChange={(e) => setExtensionsDraft(e.target.value)}
            onBlur={() => {
              const extensions = extensionsDraft
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
