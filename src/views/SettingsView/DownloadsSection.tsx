import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import { toast } from '@/lib/toast';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { SettingToggle, SettingNumberInput } from './SettingField';

interface DownloadsSectionProps {
  config: AppConfig;
}

export function DownloadsSection({ config }: DownloadsSectionProps) {
  const { t } = useTranslation();
  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
    onSuccess: () => {
      toast.success(t('settings.toast.updateSuccess'));
    },
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ patch: { [key]: value } as AppConfigPatch });
  };

  const speedLimitMb = config.speedLimitBytesPerSec
    ? config.speedLimitBytesPerSec / 1048576
    : 0;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.downloads.title')}</h2>
        <p className="text-sm text-muted-foreground">{t('settings.downloads.description')}</p>
      </div>

      <div className="space-y-1">
        <SettingNumberInput
          label={t('settings.downloads.maxConcurrent')}
          value={config.maxConcurrentDownloads}
          onChange={(v) => handleChange('maxConcurrentDownloads', v)}
          min={1}
          max={20}
        />
        <SettingNumberInput
          label={t('settings.downloads.maxSegments')}
          description={t('settings.downloads.maxSegmentsDesc')}
          value={config.maxSegmentsPerDownload}
          onChange={(v) => handleChange('maxSegmentsPerDownload', v)}
          min={1}
          max={32}
        />
        <SettingNumberInput
          label={t('settings.downloads.speedLimit')}
          description={t('settings.downloads.speedLimitDesc')}
          value={speedLimitMb}
          onChange={(v) =>
            handleChange('speedLimitBytesPerSec', v === 0 ? null : Math.round(v * 1048576))
          }
          min={0}
          step={0.5}
        />
        <SettingNumberInput
          label={t('settings.downloads.maxRetries')}
          value={config.maxRetries}
          onChange={(v) => handleChange('maxRetries', v)}
          min={0}
          max={100}
        />
        <SettingNumberInput
          label={t('settings.downloads.retryDelay')}
          value={config.retryDelaySeconds}
          onChange={(v) => handleChange('retryDelaySeconds', v)}
          min={0}
          max={3600}
        />
      </div>

      <div className="space-y-1">
        <SettingToggle
          label={t('settings.downloads.verifyChecksums')}
          description={t('settings.downloads.verifyChecksumsDesc')}
          checked={config.verifyChecksums}
          onCheckedChange={(v) => handleChange('verifyChecksums', v)}
        />
        <SettingToggle
          label={t('settings.downloads.preAllocate')}
          description={t('settings.downloads.preAllocateDesc')}
          checked={config.preAllocateSpace}
          onCheckedChange={(v) => handleChange('preAllocateSpace', v)}
        />
      </div>
    </div>
  );
}
