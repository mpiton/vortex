import { useState } from 'react';
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
  const [showApiKey, setShowApiKey] = useState(false);

  const { mutate } = useTauriMutation<AppConfig, AppConfigPatch>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ [key]: value } as AppConfigPatch);
  };

  const maskedKey = '\u2022'.repeat(32);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Remote Access</h2>
        <p className="text-sm text-muted-foreground">Web interface and API configuration</p>
      </div>

      <Card className="border-amber-500/50 bg-amber-500/5">
        <CardContent className="flex items-start gap-3 pt-0">
          <ShieldAlert className="mt-0.5 size-5 shrink-0 text-amber-500" />
          <p className="text-sm text-amber-700 dark:text-amber-400">
            Enabling remote access exposes your download manager to the network.
            Make sure to use a strong API key and restrict access to trusted networks.
          </p>
        </CardContent>
      </Card>

      <div className="space-y-1">
        <SettingToggle
          label="Web interface"
          description="Enable the browser-based control panel"
          checked={config.webInterfaceEnabled}
          onCheckedChange={(v) => handleChange('webInterfaceEnabled', v)}
        />

        {config.webInterfaceEnabled && (
          <SettingNumberInput
            label="Web interface port"
            value={config.webInterfacePort}
            onChange={(v) => handleChange('webInterfacePort', v)}
            min={1024}
            max={65535}
          />
        )}

        <SettingToggle
          label="REST API"
          description="Enable the HTTP REST API for third-party integrations"
          checked={config.restApiEnabled}
          onCheckedChange={(v) => handleChange('restApiEnabled', v)}
        />

        <SettingToggle
          label="WebSocket"
          description="Enable real-time WebSocket events"
          checked={config.websocketEnabled}
          onCheckedChange={(v) => handleChange('websocketEnabled', v)}
        />
      </div>

      {config.restApiEnabled && (
        <div className="space-y-2">
          <p className="text-sm font-medium">API Key</p>
          <div className="flex gap-2">
            <Input
              readOnly
              value={showApiKey ? config.apiKey : maskedKey}
              className="flex-1 font-mono text-xs"
            />
            <Button
              variant="outline"
              size="icon"
              aria-label={showApiKey ? 'Hide API key' : 'Show API key'}
              onClick={() => setShowApiKey((v) => !v)}
            >
              {showApiKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </Button>
            <Button
              variant="outline"
              size="icon"
              aria-label="Copy API key"
              onClick={() => navigator.clipboard.writeText(config.apiKey)}
            >
              <Copy className="size-4" />
            </Button>
            <Button
              variant="outline"
              size="icon"
              aria-label="Regenerate API key"
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
