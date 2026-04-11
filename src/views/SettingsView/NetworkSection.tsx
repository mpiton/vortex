import { useState, useEffect } from 'react';
import { useTauriMutation } from '@/api/hooks';
import type { AppConfig, AppConfigPatch, ProxyType } from '@/types/settings';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SettingToggle, SettingNumberInput } from './SettingField';

interface NetworkSectionProps {
  config: AppConfig;
}

export function NetworkSection({ config }: NetworkSectionProps) {
  const { mutate } = useTauriMutation<AppConfig, { patch: AppConfigPatch }>('settings_update', {
    invalidateKeys: [['settings_get']],
  });

  const handleChange = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    mutate({ patch: { [key]: value } as AppConfigPatch });
  };

  const [userAgentDraft, setUserAgentDraft] = useState(config.userAgent);
  const [proxyUrlDraft, setProxyUrlDraft] = useState(config.proxyUrl ?? '');

  useEffect(() => { setUserAgentDraft(config.userAgent); }, [config.userAgent]);
  useEffect(() => { setProxyUrlDraft(config.proxyUrl ?? ''); }, [config.proxyUrl]);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Network</h2>
        <p className="text-sm text-muted-foreground">Proxy and connection settings</p>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between gap-4 py-2">
          <div>
            <p className="text-sm font-medium">Proxy type</p>
          </div>
          <Select
            value={config.proxyType}
            onValueChange={(v: ProxyType) => handleChange('proxyType', v)}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">None</SelectItem>
              <SelectItem value="http">HTTP</SelectItem>
              <SelectItem value="socks5">SOCKS5</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {config.proxyType !== 'none' && (
          <div className="space-y-1">
            <p className="text-sm font-medium">Proxy URL</p>
            <Input
              value={proxyUrlDraft}
              placeholder="http://proxy:8080"
              onChange={(e) => setProxyUrlDraft(e.target.value)}
              onBlur={() => handleChange('proxyUrl', proxyUrlDraft || null)}
            />
          </div>
        )}

        <div className="space-y-1">
          <p className="text-sm font-medium">User agent</p>
          <Input
            value={userAgentDraft}
            onChange={(e) => setUserAgentDraft(e.target.value)}
            onBlur={() => handleChange('userAgent', userAgentDraft)}
          />
        </div>

        <SettingToggle
          label="DNS over HTTPS"
          description="Use encrypted DNS queries"
          checked={config.dnsOverHttps}
          onCheckedChange={(v) => handleChange('dnsOverHttps', v)}
        />

        <SettingNumberInput
          label="Connection timeout (seconds)"
          value={config.connectionTimeoutSeconds}
          onChange={(v) => handleChange('connectionTimeoutSeconds', v)}
          min={5}
          max={300}
        />
      </div>
    </div>
  );
}
