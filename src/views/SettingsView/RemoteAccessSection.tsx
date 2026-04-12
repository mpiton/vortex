import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch } from '@/types/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Copy, Eye, EyeOff, RefreshCw, ShieldAlert } from 'lucide-react';
import { SettingToggle, SettingNumberInput } from './SettingField';

interface RemoteAccessSectionProps {
  config: AppConfig;
}

export function RemoteAccessSection({ config }: RemoteAccessSectionProps) {
  const { t } = useTranslation();
  const [showApiKey, setShowApiKey] = useState(false);

  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ patch: { [key]: value } as AppConfigPatch });
  };

  const maskedKey = '\u2022'.repeat(32);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.remote.title')}</h2>
        <p className="text-sm text-muted-foreground">{t('settings.remote.description')}</p>
      </div>

      <Card className="border-amber-500/50 bg-amber-500/5">
        <CardContent className="flex items-start gap-3 pt-0">
          <ShieldAlert className="mt-0.5 size-5 shrink-0 text-amber-500" />
          <p className="text-sm text-amber-700 dark:text-amber-400">
            {t('settings.remote.warning')}
          </p>
        </CardContent>
      </Card>

      <div className="space-y-1">
        <SettingToggle
          label={t('settings.remote.webInterface')}
          description={t('settings.remote.webInterfaceDesc')}
          checked={config.webInterfaceEnabled}
          onCheckedChange={(v) => handleChange('webInterfaceEnabled', v)}
        />

        {config.webInterfaceEnabled && (
          <SettingNumberInput
            label={t('settings.remote.webInterfacePort')}
            value={config.webInterfacePort}
            onChange={(v) => handleChange('webInterfacePort', v)}
            min={1024}
            max={65535}
          />
        )}

        <SettingToggle
          label={t('settings.remote.restApi')}
          description={t('settings.remote.restApiDesc')}
          checked={config.restApiEnabled}
          onCheckedChange={(v) => handleChange('restApiEnabled', v)}
        />

        <SettingToggle
          label={t('settings.remote.websocket')}
          description={t('settings.remote.websocketDesc')}
          checked={config.websocketEnabled}
          onCheckedChange={(v) => handleChange('websocketEnabled', v)}
        />
      </div>

      {config.restApiEnabled && (
        <div className="space-y-2">
          <p className="text-sm font-medium">{t('settings.remote.apiKey')}</p>
          <div className="flex gap-2">
            <Input
              readOnly
              value={showApiKey ? config.apiKey : maskedKey}
              className="flex-1 font-mono text-xs"
            />
            <Button
              variant="outline"
              size="icon"
              aria-label={showApiKey ? t('settings.remote.hideApiKey') : t('settings.remote.showApiKey')}
              onClick={() => setShowApiKey((v) => !v)}
            >
              {showApiKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </Button>
            <Button
              variant="outline"
              size="icon"
              aria-label={t('settings.remote.copyApiKey')}
              onClick={() => navigator.clipboard.writeText(config.apiKey)}
            >
              <Copy className="size-4" />
            </Button>
            <Button
              variant="outline"
              size="icon"
              aria-label={t('settings.remote.regenerateApiKey')}
              onClick={() => handleChange('apiKey', crypto.randomUUID())}
            >
              <RefreshCw className="size-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
