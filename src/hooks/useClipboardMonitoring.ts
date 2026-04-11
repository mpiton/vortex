import { useState, useCallback, useEffect } from 'react';
import { useTauriEvent } from '@/hooks/useTauriEvent';
import { useTauriMutation } from '@/api/hooks';
import type { ClipboardMonitoringChangedPayload } from '@/types/events';

export function useClipboardMonitoring(initialEnabled = false) {
  const [isEnabled, setIsEnabled] = useState(initialEnabled);

  // Sync with store when config loads asynchronously after mount
  useEffect(() => {
    setIsEnabled(initialEnabled);
  }, [initialEnabled]);

  useTauriEvent<ClipboardMonitoringChangedPayload>(
    'clipboard-monitoring-changed',
    useCallback((payload) => {
      setIsEnabled(payload.enabled);
    }, [])
  );

  const toggleMutation = useTauriMutation<boolean, { enabled: boolean }>(
    'clipboard_toggle'
  );

  const toggle = useCallback(
    (enabled: boolean) => {
      toggleMutation.mutate(
        { enabled },
        { onSuccess: (confirmed) => setIsEnabled(confirmed) }
      );
    },
    [toggleMutation]
  );

  return { isEnabled, toggle };
}
