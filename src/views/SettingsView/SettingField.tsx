import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';

interface SettingToggleProps {
  label: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export function SettingToggle({ label, description, checked, onCheckedChange }: SettingToggleProps) {
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <p className="text-sm font-medium">{label}</p>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
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
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <p className="text-sm font-medium">{label}</p>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      <Input
        type="number"
        className="w-24"
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </div>
  );
}
