import { useRef } from "react";
import { useVisible } from "./useVisible";

/**
 * A blueprint of a graft: grid paper, the tree drawn as linework with
 * dimension lines, the delta as a cut-away, a little formula and a title
 * block. Strokes draw themselves in when it first scrolls into view.
 * Tree outlines are the paths from public/tree.svg.
 */
const TREE = [
  "M140.436 58.35L357.328 198.152L284.483 212.139L267.717 278.916L214.414 253.944L208.99 342.926L161.282 279.248L109.153 354.43L112.281 314.853Z",
  "M167.638 161.471L399.072 326.611L317.167 338.049L338.632 414.835L253.203 386.427L224.317 477.571L175.666 405.098L112.588 484.832L118.775 441.552Z",
  "M234.751 292.768L448.9 577.373L346.743 560.129L343.409 659.955L252.078 593.784L183.177 691.762L152.407 586.828L46.969 658.275L70.692 608.908Z",
];

export function BlueprintSheet({ className }: { className?: string }) {
  const host = useRef<SVGSVGElement>(null);
  const seen = useVisible(host, true);
  return (
    <svg
      ref={host}
      className={`blueprint ${seen ? "bp-draw" : "bp-wait"}${className ? ` ${className}` : ""}`}
      viewBox="0 0 480 300"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
    >
      <defs>
        <pattern id="bp-fine" width="10" height="10" patternUnits="userSpaceOnUse">
          <path d="M10 0H0V10" className="bp-grid-fine" />
        </pattern>
        <pattern id="bp-grid" width="50" height="50" patternUnits="userSpaceOnUse">
          <rect width="50" height="50" fill="url(#bp-fine)" />
          <path d="M50 0H0V50" className="bp-grid" />
        </pattern>
        <marker id="bp-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0 0L8 4L0 8" className="bp-head" />
        </marker>
      </defs>
      <rect width="480" height="300" className="bp-paper" />
      <rect width="480" height="300" fill="url(#bp-grid)" />

      {/* the tree, scaled into the right half */}
      <g transform="translate(286 22) scale(0.36)">
        {TREE.map((d, i) => (
          <path key={i} d={d} pathLength={1} className={`bp-line bp-d${i}`} />
        ))}
        <path d="M228 575h46v134.6h-46z" pathLength={1} className="bp-line bp-d3" />
        <path d="M140 58 L140 20 M449 577 L520 577 M47 709 L47 760 M449 709 L449 760" pathLength={1} className="bp-thin bp-d4" />
      </g>
      {/* dimensions */}
      <path d="M440 30V278" pathLength={1} className="bp-thin bp-d4" markerStart="url(#bp-arrow)" markerEnd="url(#bp-arrow)" />
      <path d="M303 290H448" pathLength={1} className="bp-thin bp-d4" markerStart="url(#bp-arrow)" markerEnd="url(#bp-arrow)" />
      <text x="452" y="158" className="bp-text bp-fade" transform="rotate(90 452 158)">ROOT · 0ff6509</text>
      <text x="350" y="286" className="bp-text bp-fade">Δ only</text>

      {/* left: grafts orbiting one root */}
      <circle cx="110" cy="120" r="34" pathLength={1} className="bp-line bp-d0" />
      <path d="M96 112c6-10 22-10 28 0s-4 18-14 16-18-6-14-16z" pathLength={1} className="bp-hatch bp-d1" />
      <circle cx="110" cy="120" r="66" pathLength={1} className="bp-dash bp-d2" />
      {[
        [176, 120],
        [77, 63],
        [77, 177],
      ].map(([x, y], i) => (
        <g key={i}>
          <circle cx={x} cy={y} r="13" pathLength={1} className={`bp-line bp-d${i + 2}`} />
          <path d={`M${x - 6} ${y}h12M${x} ${y - 6}v12`} pathLength={1} className="bp-thin bp-d4" />
        </g>
      ))}
      <path d="M190 112C230 80 250 70 280 74" pathLength={1} className="bp-dash bp-d3" markerEnd="url(#bp-arrow)" />

      {/* formula */}
      <text x="40" y="232" className="bp-math bp-fade">cost(graft) = |Δ|</text>
      <text x="40" y="254" className="bp-math small bp-fade">port.app = {"{{port.app}}"}</text>

      {/* title block */}
      <g className="bp-fade">
        <path d="M200 262H300V292H200Z M200 272H300 M250 262V292" pathLength={1} className="bp-thin" />
        <text x="205" y="270" className="bp-text">BLUEPRINT</text>
        <text x="255" y="270" className="bp-text">SHEET 1/1</text>
        <text x="205" y="286" className="bp-text">graft.toml</text>
        <text x="255" y="286" className="bp-text">[web]</text>
      </g>
    </svg>
  );
}
