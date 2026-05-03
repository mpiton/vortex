import { useState, useId, useEffect } from "react";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";

interface SettingToggleProps {
  label: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export function SettingToggle({
  label,
  description,
  checked,
  onCheckedChange,
}: SettingToggleProps) {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}

interface SettingNumberInputProps {
  label: string;
  description?: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
}

export function SettingNumberInput({
  label,
  description,
  value,
  onChange,
  min,
  max,
  step,
}: SettingNumberInputProps) {
  const id = useId();
  const [localValue, setLocalValue] = useState(String(value));

  useEffect(() => {
    setLocalValue(String(value));
  }, [value]);

  const commit = () => {
    const num = Number(localValue);
    if (!Number.isNaN(num) && localValue !== "") {
      let clamped = Math.min(max ?? num, Math.max(min ?? num, num));
      if (step && step > 0) {
        const base = min ?? 0;
        clamped = Math.round((clamped - base) / step) * step + base;
      }
      setLocalValue(String(clamped));
      onChange(clamped);
    } else {
      setLocalValue(String(value));
    }
  };

  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      <Input
        id={id}
        type="number"
        className="w-24"
        value={localValue}
        min={min}
        max={max}
        step={step}
        onChange={(e) => setLocalValue(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
        }}
      />
    </div>
  );
}
