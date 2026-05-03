import { useMemo } from "react";
import { Cell, Legend, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";
import type { HostStats } from "@/types/download";
import { CHART_TOOLTIP_BG, CHART_TOOLTIP_BORDER, paletteColor } from "./chartColors";
import { formatBytes } from "./format";

const MAX_SLICES = 10;

export interface TopHostsChartProps {
  data: HostStats[];
  ariaLabel: string;
  bytesLabel: string;
}

const TOOLTIP_STYLE = {
  background: CHART_TOOLTIP_BG,
  border: `1px solid ${CHART_TOOLTIP_BORDER}`,
  borderRadius: "6px",
  fontSize: "12px",
} as const;

export function TopHostsChart({ data, ariaLabel, bytesLabel }: TopHostsChartProps) {
  const slices = useMemo(
    () =>
      data.slice(0, MAX_SLICES).map((host, index) => ({
        name: host.hostname,
        value: host.totalBytes,
        color: paletteColor(index),
      })),
    [data],
  );

  return (
    <div role="img" aria-label={ariaLabel} className="h-64 w-full">
      <ResponsiveContainer width="100%" height="100%">
        <PieChart margin={{ top: 8, right: 8, bottom: 8, left: 8 }}>
          <Pie
            data={slices}
            dataKey="value"
            nameKey="name"
            innerRadius="55%"
            outerRadius="80%"
            paddingAngle={1}
          >
            {slices.map((slice) => (
              <Cell key={slice.name} fill={slice.color} stroke="transparent" />
            ))}
          </Pie>
          <Tooltip
            contentStyle={TOOLTIP_STYLE}
            formatter={(value) => [formatBytes(typeof value === "number" ? value : 0), bytesLabel]}
          />
          <Legend verticalAlign="bottom" iconType="circle" wrapperStyle={{ fontSize: "11px" }} />
        </PieChart>
      </ResponsiveContainer>
    </div>
  );
}
