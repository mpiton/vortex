import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { tauriInvoke } from "@/api/client";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { PluginConfigField } from "./PluginConfigField";
import type { PluginConfigField as ConfigField, PluginConfigView } from "@/types/plugin-config";

interface PluginConfigDialogProps {
  pluginName: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const QUERY_KEY = (name: string) => ["plugin_config_get", { name }] as const;

function validate(field: ConfigField, value: string): string | null {
  if (field.fieldType === "boolean") {
    if (value !== "true" && value !== "false") return "Invalid boolean";
    return null;
  }
  if (field.fieldType === "integer") {
    if (!/^-?\d+$/.test(value)) return "Must be an integer";
    const n = Number(value);
    if (field.min !== null && n < field.min) return `Min ${field.min}`;
    if (field.max !== null && n > field.max) return `Max ${field.max}`;
    return null;
  }
  if (field.fieldType === "float") {
    if (value.trim() === "") return "Must be a number";
    const n = Number(value);
    if (Number.isNaN(n)) return "Must be a number";
    if (field.min !== null && n < field.min) return `Min ${field.min}`;
    if (field.max !== null && n > field.max) return `Max ${field.max}`;
    return null;
  }
  if (field.fieldType === "url") {
    if (!/^https?:\/\//.test(value)) return "Must be http(s)://";
    return null;
  }
  if (field.fieldType === "enum") {
    if (!field.options.includes(value)) return "Pick one of the options";
    return null;
  }
  if (field.fieldType === "array") {
    try {
      const parsed = JSON.parse(value);
      if (!Array.isArray(parsed)) return "Must be a JSON array";
    } catch {
      return "Must be valid JSON";
    }
    return null;
  }
  if (field.fieldType === "string") {
    if (field.options.length > 0 && !field.options.includes(value)) {
      return "Pick one of the options";
    }
    if (field.regex !== null) {
      try {
        if (!new RegExp(field.regex).test(value)) return "Does not match pattern";
      } catch {
        // Malformed regex shipped by the plugin — let the backend reject it.
      }
    }
    return null;
  }
  return null;
}

export function PluginConfigDialog({
  pluginName,
  open,
  onOpenChange,
}: PluginConfigDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<Record<string, string>>({});

  const enabled = open && pluginName !== null;
  const { data, isLoading, isError } = useQuery({
    queryKey: pluginName ? QUERY_KEY(pluginName) : ["plugin_config_get_disabled"],
    queryFn: () =>
      tauriInvoke<PluginConfigView>("plugin_config_get", { name: pluginName! }),
    enabled,
  });

  const updateMutation = useTauriMutation<
    void,
    { name: string; key: string; value: string }
  >("plugin_config_update", {
    onSuccess: (_d, _v) => {
      // Refresh the form values from disk so the UI reflects the
      // canonical persisted state, not just the optimistic draft.
      if (pluginName) {
        queryClient.invalidateQueries({ queryKey: QUERY_KEY(pluginName) });
      }
    },
  });

  // Reset draft whenever a fresh schema arrives or the dialog is reopened.
  // Skip while a save is in flight: each `mutateAsync` triggers
  // `invalidateQueries`, and a refetch landing mid-save would otherwise
  // overwrite the draft and feed stale values into the next update.
  useEffect(() => {
    if (data && !updateMutation.isPending) setDraft(data.values);
  }, [data, updateMutation.isPending]);

  const errors = useMemo(() => {
    if (!data) return {} as Record<string, string>;
    const out: Record<string, string> = {};
    for (const field of data.fields) {
      const value = draft[field.key] ?? "";
      const err = validate(field, value);
      if (err) out[field.key] = err;
    }
    return out;
  }, [data, draft]);

  async function handleSave() {
    if (!pluginName || !data) return;
    if (Object.keys(errors).length > 0) {
      toast.error(t("plugins.config.toast.validationFailed"));
      return;
    }

    const changedKeys = data.fields
      .map((f) => f.key)
      .filter((key) => draft[key] !== data.values[key]);

    if (changedKeys.length === 0) {
      onOpenChange(false);
      return;
    }

    try {
      for (const key of changedKeys) {
        await updateMutation.mutateAsync({
          name: pluginName,
          key,
          value: draft[key],
        });
      }
      toast.success(t("plugins.config.toast.saveSuccess"));
      onOpenChange(false);
    } catch {
      // useTauriMutation already surfaces a toast on each error.
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("plugins.config.title", { name: pluginName ?? "" })}
          </DialogTitle>
          <DialogDescription>
            {t("plugins.config.description")}
          </DialogDescription>
        </DialogHeader>

        {isLoading && (
          <p className="text-xs text-text-dim py-4">
            {t("plugins.config.loading")}
          </p>
        )}
        {isError && (
          <p className="text-xs text-destructive py-4" role="alert">
            {t("plugins.config.error")}
          </p>
        )}
        {data && data.fields.length === 0 && (
          <p className="text-xs text-text-dim py-4">
            {t("plugins.config.noFields")}
          </p>
        )}
        {data && data.fields.length > 0 && (
          <div className="flex flex-col gap-3 py-2 max-h-[60vh] overflow-y-auto">
            {data.fields.map((field) => (
              <PluginConfigField
                key={field.key}
                field={field}
                value={draft[field.key] ?? ""}
                onChange={(value) =>
                  setDraft((prev) => ({ ...prev, [field.key]: value }))
                }
                errorMessage={errors[field.key]}
              />
            ))}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            onClick={handleSave}
            disabled={
              !data || Object.keys(errors).length > 0 || updateMutation.isPending
            }
          >
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
