import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import { toast } from '@/lib/toast';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { FolderOpen } from 'lucide-react';
import { SettingToggle } from './SettingField';

interface GeneralSectionProps {
  config: AppConfig;
}

export function GeneralSection({ config }: GeneralSectionProps) {
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

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.general.title')}</h2>
        <p className="text-sm text-muted-foreground">{t('settings.general.description')}</p>
      </div>

      <div className="space-y-1">
        <p className="text-sm font-medium">{t('settings.general.downloadDir')}</p>
        <div className="flex gap-2">
          <Input
            readOnly
            value={config.downloadDir ?? ''}
            placeholder={t('settings.general.downloadDirPlaceholder')}
            className="flex-1"
          />
          <Button variant="outline" size="icon" aria-label={t('settings.general.browse')} disabled>
            <FolderOpen className="size-4" />
          </Button>
        </div>
      </div>

      <div className="space-y-1">
        <SettingToggle
          label={t('settings.general.startMinimized')}
          description={t('settings.general.startMinimizedDesc')}
          checked={config.startMinimized}
          onCheckedChange={(v) => handleChange('startMinimized', v)}
        />
        <SettingToggle
          label={t('settings.general.notifications')}
          description={t('settings.general.notificationsDesc')}
          checked={config.notificationsEnabled}
          onCheckedChange={(v) => handleChange('notificationsEnabled', v)}
        />
        <SettingToggle
          label={t('settings.general.autoExtract')}
          description={t('settings.general.autoExtractDesc')}
          checked={config.autoExtract}
          onCheckedChange={(v) => handleChange('autoExtract', v)}
        />
        <SettingToggle
          label={t('settings.general.clipboardMonitoring')}
          description={t('settings.general.clipboardMonitoringDesc')}
          checked={config.clipboardMonitoring}
          onCheckedChange={(v) => handleChange('clipboardMonitoring', v)}
        />
        <SettingToggle
          label={t('settings.general.soundEffects')}
          description={t('settings.general.soundEffectsDesc')}
          checked={config.soundEnabled}
          onCheckedChange={(v) => handleChange('soundEnabled', v)}
        />
        <SettingToggle
          label={t('settings.general.confirmDelete')}
          description={t('settings.general.confirmDeleteDesc')}
          checked={config.confirmDelete}
          onCheckedChange={(v) => handleChange('confirmDelete', v)}
        />
        <SettingToggle
          label={t('settings.general.subfolderPerPackage')}
          description={t('settings.general.subfolderPerPackageDesc')}
          checked={config.subfolderPerPackage}
          onCheckedChange={(v) => handleChange('subfolderPerPackage', v)}
        />
      </div>
    </div>
  );
}
