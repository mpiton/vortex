import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { FolderOpen } from 'lucide-react';
import { SettingToggle } from './SettingField';

interface GeneralSectionProps {
  config: AppConfig;
}

export function GeneralSection({ config }: GeneralSectionProps) {
  const { mutate } = useTauriMutation<AppConfig, AppConfigPatch>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ [key]: value } as AppConfigPatch);
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">General</h2>
        <p className="text-sm text-muted-foreground">Basic application settings</p>
      </div>

      <div className="space-y-1">
        <p className="text-sm font-medium">Download directory</p>
        <div className="flex gap-2">
          <Input
            readOnly
            value={config.downloadDir ?? ''}
            placeholder="Default download directory"
            className="flex-1"
          />
          <Button variant="outline" size="icon" aria-label="Browse">
            <FolderOpen className="size-4" />
          </Button>
        </div>
      </div>

      <div className="space-y-1">
        <SettingToggle
          label="Start minimized"
          description="Start the app minimized to the system tray"
          checked={config.startMinimized}
          onCheckedChange={(v) => handleChange('startMinimized', v)}
        />
        <SettingToggle
          label="Notifications"
          description="Show desktop notifications for completed downloads"
          checked={config.notificationsEnabled}
          onCheckedChange={(v) => handleChange('notificationsEnabled', v)}
        />
        <SettingToggle
          label="Auto extract"
          description="Automatically extract archives after download"
          checked={config.autoExtract}
          onCheckedChange={(v) => handleChange('autoExtract', v)}
        />
        <SettingToggle
          label="Clipboard monitoring"
          description="Watch clipboard for downloadable links"
          checked={config.clipboardMonitoring}
          onCheckedChange={(v) => handleChange('clipboardMonitoring', v)}
        />
        <SettingToggle
          label="Sound effects"
          description="Play sounds on download events"
          checked={config.soundEnabled}
          onCheckedChange={(v) => handleChange('soundEnabled', v)}
        />
        <SettingToggle
          label="Confirm before delete"
          description="Ask for confirmation before deleting downloads"
          checked={config.confirmDelete}
          onCheckedChange={(v) => handleChange('confirmDelete', v)}
        />
        <SettingToggle
          label="Subfolder per package"
          description="Create a separate folder for each download package"
          checked={config.subfolderPerPackage}
          onCheckedChange={(v) => handleChange('subfolderPerPackage', v)}
        />
      </div>
    </div>
  );
}
