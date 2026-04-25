import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { PluginConfigField as ConfigField } from "@/types/plugin-config";

interface PluginConfigFieldProps {
  field: ConfigField;
  value: string;
  onChange: (value: string) => void;
  errorMessage?: string;
}

function isEnumLike(field: ConfigField): boolean {
  if (field.fieldType === "enum") return true;
  return field.fieldType === "string" && field.options.length > 0;
}

export function PluginConfigField({
  field,
  value,
  onChange,
  errorMessage,
}: PluginConfigFieldProps) {
  const labelId = `plugin-config-field-${field.key}`;
  const describedBy = errorMessage ? `${labelId}-error` : undefined;

  return (
    <div className="flex flex-col gap-1.5">
      <label
        id={labelId}
        htmlFor={field.key}
        className="text-xs font-medium text-foreground"
      >
        {field.key}
      </label>
      {field.description && (
        <p className="text-[10px] text-text-dim">{field.description}</p>
      )}
      {renderControl(field, value, onChange, labelId, field.key, describedBy)}
      {errorMessage && (
        <p
          id={`${labelId}-error`}
          role="alert"
          className="text-[10px] text-destructive"
        >
          {errorMessage}
        </p>
      )}
    </div>
  );
}

function renderControl(
  field: ConfigField,
  value: string,
  onChange: (value: string) => void,
  labelId: string,
  inputId: string,
  describedBy: string | undefined,
) {
  if (field.fieldType === "boolean") {
    return (
      <Switch
        id={inputId}
        aria-labelledby={labelId}
        aria-describedby={describedBy}
        checked={value === "true"}
        onCheckedChange={(checked) => onChange(checked ? "true" : "false")}
      />
    );
  }

  if (isEnumLike(field)) {
    return (
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger
          id={inputId}
          aria-labelledby={labelId}
          aria-describedby={describedBy}
          className="h-8 text-xs"
        >
          <SelectValue placeholder={field.default ?? ""} />
        </SelectTrigger>
        <SelectContent>
          {field.options.map((opt) => (
            <SelectItem key={opt} value={opt}>
              {opt}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (field.fieldType === "integer" || field.fieldType === "float") {
    return (
      <Input
        id={inputId}
        aria-labelledby={labelId}
        aria-describedby={describedBy}
        type="number"
        step={field.fieldType === "integer" ? 1 : "any"}
        min={field.min ?? undefined}
        max={field.max ?? undefined}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-8 text-xs"
      />
    );
  }

  return (
    <Input
      id={inputId}
      aria-labelledby={labelId}
      aria-describedby={describedBy}
      type={field.fieldType === "url" ? "url" : "text"}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={field.default ?? ""}
      className="h-8 text-xs"
    />
  );
}
