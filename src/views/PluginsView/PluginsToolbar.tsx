import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

interface PluginsToolbarProps {
  categories: string[];
  activeCategory: string;
  onCategoryChange: (category: string) => void;
  search: string;
  onSearchChange: (value: string) => void;
}

export function PluginsToolbar({
  categories,
  activeCategory,
  onCategoryChange,
  search,
  onSearchChange,
}: PluginsToolbarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-3 px-6 py-2 bg-surface border-b border-border-soft shrink-0">
      <div role="tablist" className="flex gap-0.5 bg-surface-muted rounded-md p-0.5">
        {categories.map((category) => {
          const label = t(`plugins.categories.${category}`, {
            defaultValue: category,
          });
          const isActive = category === activeCategory;
          return (
            <button
              key={category}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => onCategoryChange(category)}
              className={
                isActive
                  ? "px-3 py-1 rounded text-[11px] font-medium bg-surface text-accent shadow-sm transition-colors"
                  : "px-3 py-1 rounded text-[11px] text-text-dim hover:text-text-muted transition-colors"
              }
            >
              {label}
            </button>
          );
        })}
      </div>

      <div className="flex-1" />

      <div className="relative w-64">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3 w-3 text-text-ghost" />
        <input
          type="search"
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={t("plugins.search.placeholder")}
          aria-label={t("plugins.search.placeholder")}
          className="w-full h-7 pl-8 pr-3 text-[11px] bg-surface-alt border border-border-soft rounded-md placeholder:text-text-ghost focus:outline-none focus:border-accent"
        />
      </div>
    </div>
  );
}
