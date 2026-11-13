import { useMemo, useRef } from "react";
import { useVisible } from "./useVisible";

/**
 * public/tree.svg cut into jigsaw pieces that fly in and lock together,
 * hold, then drift apart and do it again: Integrate, as a picture. Pieces
 * share edges, so every tab on one piece is a blank on its neighbour.
 * Runs only while on screen; assembled and still under reduced motion.
 */
const W = 457;
const H = 710;
const COLS = 3;
const ROWS = 4;

type Pt = [number, number];

// One edge from a to b; `bulge` (±1, 0 for a border) pushes a round tab out along +x or +y.
function edge(a: Pt, b: Pt, bulge: number, axis: "x" | "y") {
  const at = (t: number): Pt => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
  if (!bulge) return `L${b[0]} ${b[1]}`;
  const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
  const k = len * 0.36 * bulge;
  const off = (p: Pt): Pt => (axis === "y" ? [p[0], p[1] + k] : [p[0] + k, p[1]]);
  const [p1, p2] = [at(0.38), at(0.62)];
  const [c1, c2] = [off(at(0.22)), off(at(0.78))];
  return `L${p1[0]} ${p1[1]}C${c1[0]} ${c1[1]} ${c2[0]} ${c2[1]} ${p2[0]} ${p2[1]}L${b[0]} ${b[1]}`;
}

function pieces() {
  const w = W / COLS;
  const h = H / ROWS;
  // deterministic tab directions for each shared edge
  const hTab = (r: number, c: number) => ((r * 7 + c * 3) % 2 ? 1 : -1); // between row r-1 and r
  const vTab = (r: number, c: number) => ((r * 5 + c * 11) % 2 ? 1 : -1); // between col c-1 and c
  const out = [];
  for (let r = 0; r < ROWS; r++) {
    for (let c = 0; c < COLS; c++) {
      const x = c * w;
      const y = r * h;
      const tl: Pt = [x, y], tr: Pt = [x + w, y], br: Pt = [x + w, y + h], bl: Pt = [x, y + h];
      const d =
        `M${x} ${y}` +
        edge(tl, tr, r === 0 ? 0 : hTab(r, c), "y") +
        edge(tr, br, c === COLS - 1 ? 0 : vTab(r, c + 1), "x") +
        edge(br, bl, r === ROWS - 1 ? 0 : hTab(r + 1, c), "y") +
        edge(bl, tl, c === 0 ? 0 : vTab(r, c), "x") +
        "Z";
      const i = r * COLS + c;
      // where each piece waits before flying in
      const ang = i * 2.399; // golden angle spreads them evenly
      out.push({
        d,
        i,
        dx: Math.round(Math.cos(ang) * 260),
        dy: Math.round(Math.sin(ang) * 200),
        rot: ((i * 47) % 70) - 35,
      });
    }
  }
  return out;
}

export function TreePuzzle({ className }: { className?: string }) {
  const host = useRef<SVGSVGElement>(null);
  const visible = useVisible(host);
  const list = useMemo(pieces, []);
  return (
    <svg
      ref={host}
      className={`tree-puzzle${visible ? " is-on" : ""}${className ? ` ${className}` : ""}`}
      viewBox={`-170 -60 ${W + 340} ${H + 120}`}
      aria-hidden="true"
    >
      <defs>
        {list.map((p) => (
          <clipPath key={p.i} id={`tp-${p.i}`}>
            <path d={p.d} />
          </clipPath>
        ))}
      </defs>
      {/* the gap the pieces fill */}
      <rect width={W} height={H} rx="6" className="tp-ghost" />
      {list.map((p) => (
        <g
          key={p.i}
          className="tp-piece"
          style={{ "--dx": `${p.dx}px`, "--dy": `${p.dy}px`, "--rot": `${p.rot}deg`, "--i": p.i } as React.CSSProperties}
        >
          <g clipPath={`url(#tp-${p.i})`}>
            <rect width={W} height={H} className="tp-paper" />
            <image href="/tree.svg" width={W} height={H} />
          </g>
          <path d={p.d} className="tp-cut" />
        </g>
      ))}
    </svg>
  );
}
