import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { SettingToggle, SettingNumberInput } from './SettingField';

interface DownloadsSectionProps {
  config: AppConfig;
}

export function DownloadsSection({ config }: DownloadsSectionProps) {
  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
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
        <h2 className="text-lg font-semibold">Downloads</h2>
        <p className="text-sm text-muted-foreground">Download engine configuration</p>
      </div>

      <div className="space-y-1">
        <SettingNumberInput
          label="Max concurrent downloads"
          value={config.maxConcurrentDownloads}
          onChange={(v) => handleChange('maxConcurrentDownloads', v)}
          min={1}
          max={100}
        />
        <SettingNumberInput
          label="Max segments per download"
          description="Number of parallel connections per file"
          value={config.maxSegmentsPerDownload}
          onChange={(v) => handleChange('maxSegmentsPerDownload', v)}
          min={1}
          max={32}
        />
        <SettingNumberInput
          label="Speed limit (MB/s)"
          description="0 = unlimited"
          value={speedLimitMb}
          onChange={(v) =>
            handleChange('speedLimitBytesPerSec', v === 0 ? null : Math.round(v * 1048576))
          }
          min={0}
          step={0.5}
        />
        <SettingNumberInput
          label="Max retries"
          value={config.maxRetries}
          onChange={(v) => handleChange('maxRetries', v)}
          min={0}
          max={100}
        />
        <SettingNumberInput
          label="Retry delay (seconds)"
          value={config.retryDelaySeconds}
          onChange={(v) => handleChange('retryDelaySeconds', v)}
          min={0}
          max={3600}
        />
      </div>

      <div className="space-y-1">
        <SettingToggle
          label="Verify checksums"
          description="Verify file integrity after download"
          checked={config.verifyChecksums}
          onCheckedChange={(v) => handleChange('verifyChecksums', v)}
        />
        <SettingToggle
          label="Pre-allocate space"
          description="Reserve disk space before downloading"
          checked={config.preAllocateSpace}
          onCheckedChange={(v) => handleChange('preAllocateSpace', v)}
        />
      </div>
    </div>
  );
}
