import { useClipboardMonitoring } from '@/hooks/useClipboardMonitoring';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';

export function ClipboardIndicator() {
  const { t } = useTranslation();
  const initialEnabled = useSettingsStore(
    (s) => s.config?.clipboardMonitoring ?? false
  );
  const { isEnabled, toggle } = useClipboardMonitoring(initialEnabled);

  return (
    <button
      type="button"
      onClick={() => toggle(!isEnabled)}
      aria-pressed={isEnabled}
      className="flex items-center gap-1.5 text-[11px] text-text-dim hover:text-text transition-colors"
      title={isEnabled ? t('statusBar.clipboardActive') : t('statusBar.clipboardPaused')}
    >
      <div
        className={`h-[7px] w-[7px] rounded-full transition-colors ${
          isEnabled ? 'bg-success' : 'bg-border'
        }`}
      />
      <span>{t('statusBar.clipboard')}</span>
    </button>
  );
}
