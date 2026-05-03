import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import type { ClipboardMonitoringChangedPayload } from "@/types/events";

export function useClipboardMonitoring(initialEnabled = false) {
  const { t } = useTranslation();
  const [isEnabled, setIsEnabled] = useState(initialEnabled);

  // Sync with store when config loads asynchronously after mount
  useEffect(() => {
    setIsEnabled(initialEnabled);
  }, [initialEnabled]);

  useTauriEvent<ClipboardMonitoringChangedPayload>(
    "clipboard-monitoring-changed",
    useCallback((payload) => {
      setIsEnabled(payload.enabled);
    }, []),
  );

  const { mutate } = useTauriMutation<boolean, { enabled: boolean }>("clipboard_toggle", {
    onSuccess: (confirmed) => {
      setIsEnabled(confirmed);
      toast.success(confirmed ? t("clipboard.toast.enabled") : t("clipboard.toast.disabled"));
    },
  });

  const toggle = useCallback(
    (enabled: boolean) => {
      mutate({ enabled });
    },
    [mutate],
  );

  return { isEnabled, toggle };
}
