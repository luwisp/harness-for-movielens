import { useState } from "react";
import type { Point } from "./types";
import { groupName } from "./format";
const colors = [
  "#5377b5",
  "#c17956",
  "#658e73",
  "#ad6da0",
  "#a38c3b",
  "#6a8eaf",
  "#b26b79",
  "#6b65aa",
];
export function ScatterPlot({
  points,
  title,
  axes,
}: {
  points: Point[];
  title: string;
  axes: string[];
}) {
  const [selected, setSelected] = useState<Point | null>(null);
  const xs = points.map((p) => p.x),
    ys = points.map((p) => p.y);
  const bounds = (values: number[]) => {
    const lo = Math.min(...values),
      hi = Math.max(...values);
    return [lo, hi, Math.max(hi - lo, 1e-9)];
  };
  if (!points.length) return <p>坐标尚未生成。</p>;
  const [xmin, xmax, dx] = bounds(xs),
    [ymin, ymax, dy] = bounds(ys);
  return (
    <figure className="analysis-plot">
      <figcaption>{title}</figcaption>
      <svg viewBox="0 0 440 285" role="img" aria-label={title}>
        <line x1="48" y1="238" x2="418" y2="238" />
        <line x1="48" y1="18" x2="48" y2="238" />
        {[0, 0.5, 1].map((f) => (
          <g key={f}>
            <text x={48 + 370 * f} y="257" textAnchor="middle">
              {(xmin + dx * f).toFixed(2)}
            </text>
            <text x="42" y={238 - 220 * f} textAnchor="end">
              {(ymin + dy * f).toFixed(2)}
            </text>
          </g>
        ))}
        <text x="235" y="280" textAnchor="middle">
          {axes[0]}
        </text>
        <text x="15" y="132" transform="rotate(-90 15 132)" textAnchor="middle">
          {axes[1]}
        </text>
        {points.map((p) => (
          <circle
            key={p.user_id}
            cx={48 + (370 * (p.x - xmin)) / dx}
            cy={238 - (220 * (p.y - ymin)) / dy}
            r={selected?.user_id === p.user_id ? 5 : 2.7}
            fill={
              p.noise
                ? "#9ca5b0"
                : colors[
                    ((p.group % colors.length) + colors.length) % colors.length
                  ]
            }
            fillOpacity=".72"
            tabIndex={0}
            onFocus={() => setSelected(p)}
            onClick={() => setSelected(p)}
            aria-label={`用户 ${p.user_id}，${groupName(p.group, p.noise)}`}
          >
            <title>
              用户 {p.user_id} · {groupName(p.group, p.noise)} · (
              {p.x.toFixed(4)}, {p.y.toFixed(4)})
            </title>
          </circle>
        ))}
      </svg>
      <small>
        {selected
          ? `用户 ${selected.user_id} · ${groupName(selected.group, selected.noise)} · 坐标 (${selected.x.toFixed(4)}, ${selected.y.toFixed(4)})`
          : `${points.length} 位用户；点击或聚焦点查看坐标`}
      </small>
      {xmax === xmin && <small>横轴取值相同</small>}
      {ymax === ymin && <small>纵轴取值相同</small>}
    </figure>
  );
}
