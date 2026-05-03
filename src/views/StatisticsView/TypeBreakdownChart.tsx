import { useMemo } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { TypeBreakdownEntry } from "./derive";
import {
  CHART_AXIS_COLOR,
  CHART_GRID_COLOR,
  CHART_TOOLTIP_BG,
  CHART_TOOLTIP_BORDER,
  paletteColor,
} from "./chartColors";
import { formatBytes } from "./format";

const MAX_ROWS = 10;

export interface TypeBreakdownChartProps {
  data: TypeBreakdownEntry[];
  ariaLabel: string;
  bytesLabel: string;
}

const TOOLTIP_STYLE = {
  background: CHART_TOOLTIP_BG,
  border: `1px solid ${CHART_TOOLTIP_BORDER}`,
  borderRadius: "6px",
  fontSize: "12px",
} as const;

export function TypeBreakdownChart({ data, ariaLabel, bytesLabel }: TypeBreakdownChartProps) {
  const rows = useMemo(
    () =>
      data.slice(0, MAX_ROWS).map((entry, index) => ({
        extension: entry.extension,
        bytes: entry.bytes,
        color: paletteColor(index),
      })),
    [data],
  );

  return (
    <div role="img" aria-label={ariaLabel} className="h-64 w-full">
      <ResponsiveContainer width="100%" height="100%">
        <BarChart layout="vertical" data={rows} margin={{ top: 8, right: 16, bottom: 8, left: 24 }}>
          <CartesianGrid stroke={CHART_GRID_COLOR} strokeDasharray="3 3" horizontal={false} />
          <XAxis
            type="number"
            stroke={CHART_AXIS_COLOR}
            fontSize={11}
            tickLine={false}
            axisLine={false}
            tickFormatter={(value: number) => formatBytes(value)}
          />
          <YAxis
            type="category"
            dataKey="extension"
            stroke={CHART_AXIS_COLOR}
            fontSize={11}
            tickLine={false}
            axisLine={false}
            width={56}
          />
          <Tooltip
            cursor={{ fill: "transparent" }}
            contentStyle={TOOLTIP_STYLE}
            formatter={(value) => [formatBytes(typeof value === "number" ? value : 0), bytesLabel]}
          />
          <Bar dataKey="bytes" radius={[0, 4, 4, 0]}>
            {rows.map((row) => (
              <Cell key={row.extension} fill={row.color} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
