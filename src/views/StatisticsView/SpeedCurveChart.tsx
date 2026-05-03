import { useMemo } from "react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ACCENT_COLOR,
  CHART_AXIS_COLOR,
  CHART_GRID_COLOR,
  CHART_TOOLTIP_BG,
  CHART_TOOLTIP_BORDER,
} from "./chartColors";
import type { SpeedPoint } from "./derive";
import { formatSpeed } from "./format";

export interface SpeedCurveChartProps {
  data: SpeedPoint[];
  ariaLabel: string;
  yAxisLabel: string;
  xAxisLabel: string;
}

const TOOLTIP_STYLE = {
  background: CHART_TOOLTIP_BG,
  border: `1px solid ${CHART_TOOLTIP_BORDER}`,
  borderRadius: "6px",
  fontSize: "12px",
} as const;

export function SpeedCurveChart({ data, ariaLabel, yAxisLabel, xAxisLabel }: SpeedCurveChartProps) {
  const points = useMemo(() => data.map((p) => ({ date: p.date, avgSpeed: p.avgSpeed })), [data]);

  return (
    <div role="img" aria-label={ariaLabel} className="h-64 w-full">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={points} margin={{ top: 8, right: 12, bottom: 8, left: 4 }}>
          <CartesianGrid stroke={CHART_GRID_COLOR} strokeDasharray="3 3" vertical={false} />
          <XAxis
            dataKey="date"
            stroke={CHART_AXIS_COLOR}
            fontSize={11}
            tickLine={false}
            axisLine={false}
            label={{ value: xAxisLabel, position: "insideBottom", offset: -2, fontSize: 11 }}
          />
          <YAxis
            stroke={CHART_AXIS_COLOR}
            fontSize={11}
            tickLine={false}
            axisLine={false}
            tickFormatter={(value: number) => formatSpeed(value)}
            label={{
              value: yAxisLabel,
              angle: -90,
              position: "insideLeft",
              fontSize: 11,
              offset: 10,
            }}
          />
          <Tooltip
            contentStyle={TOOLTIP_STYLE}
            formatter={(value) => [formatSpeed(typeof value === "number" ? value : 0), yAxisLabel]}
          />
          <Line
            type="monotone"
            dataKey="avgSpeed"
            stroke={ACCENT_COLOR}
            strokeWidth={2}
            dot={{ r: 3, fill: ACCENT_COLOR }}
            activeDot={{ r: 5 }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
