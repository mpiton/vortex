import { ChevronDown, ChevronUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { ResolutionTier } from "@/types/settings";

/**
 * All three tiers are always present: the backend appends any tier the
 * user left out, so hiding one here would only desync the two sides.
 */
const TIERS: readonly ResolutionTier[] = ["premium", "debrid", "free"];

interface ResolutionOrderSettingProps {
  value: ResolutionTier[];
  onChange: (next: ResolutionTier[]) => void;
}

export function ResolutionOrderSetting({ value, onChange }: ResolutionOrderSettingProps) {
  const { t } = useTranslation();
  const order = normalizeOrder(value);

  const move = (index: number, offset: -1 | 1) => {
    const target = index + offset;
    if (target < 0 || target >= order.length) return;
    const next = [...order];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };

  return (
    <div className="space-y-2">
      <div>
        <p className="text-sm font-medium">{t("settings.downloads.resolutionOrder.label")}</p>
        <p className="text-sm text-muted-foreground">
          {t("settings.downloads.resolutionOrder.description")}
        </p>
      </div>
      <ol className="space-y-2">
        {order.map((tier, index) => {
          const label = t(`settings.downloads.resolutionOrder.tiers.${tier}`);
          return (
            <li className="flex items-center gap-3 rounded-md border p-3" key={tier}>
              <span className="w-5 text-sm text-muted-foreground">{index + 1}</span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">{label}</p>
                <p className="text-xs text-muted-foreground">
                  {t(`settings.downloads.resolutionOrder.hints.${tier}`)}
                </p>
              </div>
              <Button
                aria-label={t("settings.downloads.resolutionOrder.moveUp", { tier: label })}
                disabled={index === 0}
                onClick={() => move(index, -1)}
                size="icon"
                type="button"
                variant="ghost"
              >
                <ChevronUp />
              </Button>
              <Button
                aria-label={t("settings.downloads.resolutionOrder.moveDown", { tier: label })}
                disabled={index === order.length - 1}
                onClick={() => move(index, 1)}
                size="icon"
                type="button"
                variant="ghost"
              >
                <ChevronDown />
              </Button>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** Mirrors `domain::model::config::normalize_resolution_order`. */
function normalizeOrder(raw: readonly ResolutionTier[]): ResolutionTier[] {
  const seen = new Set<ResolutionTier>();
  for (const tier of [...raw, ...TIERS]) {
    if (TIERS.includes(tier)) seen.add(tier);
  }
  return [...seen];
}
