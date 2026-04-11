import { useState, useEffect } from 'react';
import { Settings2, Download, Globe, Link, MonitorSmartphone, Palette } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { useTauriQuery } from '@/api/hooks';
import type { AppConfig, SettingTab } from '@/types/settings';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Skeleton } from '@/components/ui/skeleton';
import { GeneralSection } from './GeneralSection';
import { DownloadsSection } from './DownloadsSection';
import { NetworkSection } from './NetworkSection';
import { RemoteAccessSection } from './RemoteAccessSection';
import { BrowserSection } from './BrowserSection';
import { AppearanceSection } from './AppearanceSection';

const TABS: { id: SettingTab; label: string; icon: typeof Settings2 }[] = [
  { id: 'general', label: 'General', icon: Settings2 },
  { id: 'downloads', label: 'Downloads', icon: Download },
  { id: 'network', label: 'Network', icon: Globe },
  { id: 'remote', label: 'Remote Access', icon: Link },
  { id: 'browser', label: 'Browser', icon: MonitorSmartphone },
  { id: 'appearance', label: 'Appearance', icon: Palette },
];

function SectionContent({ tab, config }: { tab: SettingTab; config: AppConfig }) {
  switch (tab) {
    case 'general':
      return <GeneralSection config={config} />;
    case 'downloads':
      return <DownloadsSection config={config} />;
    case 'network':
      return <NetworkSection config={config} />;
    case 'remote':
      return <RemoteAccessSection config={config} />;
    case 'browser':
      return <BrowserSection config={config} />;
    case 'appearance':
      return <AppearanceSection config={config} />;
  }
}

export function SettingsView() {
  const [activeTab, setActiveTab] = useState<SettingTab>('general');
  const queryClient = useQueryClient();

  const { data: config, isLoading } = useTauriQuery<AppConfig>(
    'settings_get',
    undefined,
    { queryKey: ['settings_get'], staleTime: 30_000 },
  );

  // Invalidate cache when settings change from another source (e.g. clipboard toggle)
  useEffect(() => {
    const unlisten = listen('settings-updated', () => {
      queryClient.invalidateQueries({ queryKey: ['settings_get'] });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);

  if (isLoading || !config) {
    return (
      <div className="flex h-full gap-4 p-4">
        <div className="flex w-48 flex-col gap-1">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-9 w-full" />
          ))}
        </div>
        <div className="flex-1 space-y-4">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-64 w-full" />
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full gap-4 p-4">
      <nav className="flex w-48 shrink-0 flex-col gap-1">
        {TABS.map(({ id, label, icon: Icon }) => (
          <Button
            key={id}
            variant={activeTab === id ? 'default' : 'ghost'}
            className="justify-start gap-2"
            onClick={() => setActiveTab(id)}
          >
            <Icon className="size-4" />
            {label}
          </Button>
        ))}
      </nav>
      <ScrollArea className="flex-1">
        <div className="max-w-2xl pr-4">
          <SectionContent tab={activeTab} config={config} />
        </div>
      </ScrollArea>
    </div>
  );
}
