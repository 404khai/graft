import { useEffect, useRef, useState } from "react";
import { useVisible } from "../components/art/useVisible";
import { fmtValue, type Chart } from "./posts";

/**
 * Horizontal bar chart for posts, in the spirit of Bklit's composable charts
 * but with no chart library or Tailwind: one row per method, a value label,
 * an optional log scale and bars that grow in when the chart enters the
 * screen. Without JavaScript the bars are drawn at full length.
 */
export function BarChart({ chart }: { chart: Chart }) {
  const host = useRef<HTMLElement>(null);
  const visible = useVisible(host, true);
  const [armed, setArmed] = useState(false);
  useEffect(() => setArmed(!matchMedia("(prefers-reduced-motion: reduce)").matches), []);

  const values = chart.bars.map((b) => b.value);
  const max = Math.max(...values);
  const min = Math.min(...values);
  // On a log scale the smallest bar still gets a visible stub.
  const lo = chart.log ? min / 4 : 0;
  const width = (v: number) =>
    chart.log ? (Math.log10(v) - Math.log10(lo)) / (Math.log10(max) - Math.log10(lo)) : v / max;

  return (
    <figure ref={host} className={`bar-chart${armed ? " is-armed" : ""}${visible ? " is-on" : ""}`}>
      <figcaption className="bar-chart-title">
        {chart.title}
        {chart.log && <span>log scale</span>}
      </figcaption>
      <div className="bar-chart-rows" role="list">
        {chart.bars.map((b, i) => (
          <div key={b.label} className={`bar-row tone-${b.tone}`} role="listitem" style={{ "--i": i } as React.CSSProperties}>
            <span className="bar-label">{b.label}</span>
            <span className="bar-track">
              <i style={{ width: `${Math.max(1.5, width(b.value) * 100)}%` }} />
            </span>
            <b className="bar-value">{fmtValue(b.value, chart.unit)}</b>
          </div>
        ))}
      </div>
      {chart.note && <p className="bar-chart-note">{chart.note}</p>}
    </figure>
  );
}
