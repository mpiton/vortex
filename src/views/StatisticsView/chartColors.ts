export const ACCENT_COLOR = "var(--color-accent)";
export const ACCENT_HOVER_COLOR = "var(--color-accent-hover)";
export const ACCENT_MEDIUM_COLOR = "var(--color-accent-medium)";

export const CHART_PALETTE = [
  ACCENT_COLOR,
  ACCENT_HOVER_COLOR,
  ACCENT_MEDIUM_COLOR,
  "var(--color-cyan)",
  "var(--color-success)",
  "var(--color-warning)",
  "var(--color-error)",
  "var(--color-muted)",
  "var(--color-accent-foreground)",
  "var(--color-sidebar-icon)",
];

export function paletteColor(index: number): string {
  return CHART_PALETTE[index % CHART_PALETTE.length];
}

export const CHART_GRID_COLOR = "var(--color-surface-muted)";
export const CHART_AXIS_COLOR = "var(--color-muted-foreground)";
export const CHART_TOOLTIP_BG = "var(--color-surface)";
export const CHART_TOOLTIP_BORDER = "var(--color-surface-muted)";
