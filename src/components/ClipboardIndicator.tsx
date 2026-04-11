import { useClipboardMonitoring } from '@/hooks/useClipboardMonitoring';
import { useSettingsStore } from '@/stores/settingsStore';

export function ClipboardIndicator() {
  const initialEnabled = useSettingsStore(
    (s) => s.config?.clipboardMonitoring ?? false
  );
  const { isEnabled, toggle } = useClipboardMonitoring(initialEnabled);

  return (
    <button
      type="button"
      onClick={() => toggle(!isEnabled)}
      className="flex items-center gap-1.5 text-[11px] text-text-dim hover:text-text transition-colors"
      title={isEnabled ? 'Clipboard monitoring active' : 'Clipboard monitoring paused'}
    >
      <div
        className={`h-[7px] w-[7px] rounded-full transition-colors ${
          isEnabled ? 'bg-success' : 'bg-border'
        }`}
      />
      <span>Clipboard</span>
    </button>
  );
}
