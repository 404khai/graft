import { useEffect, useRef } from "react";
import { useVisible } from "./useVisible";

/**
 * An ordered-dither (Bayer 8×8) field of procedurally grown trees, after
 * dithercn.dev: each frame paints a small greyscale scene (trunks, branches
 * and canopies swaying in a light breeze, with a slow light sweeping across)
 * and prints it as square dots in one ink colour. Trees grow in on first
 * sight, pause off screen, and hold still under prefers-reduced-motion.
 */

export type TreeSpec = {
  /** base of the trunk, as a fraction of the field */
  x: number;
  y?: number;
  /** trunk length as a fraction of the field height */
  size: number;
  /** lean in radians; negative leans left */
  lean?: number;
  seed: number;
  depth?: number;
};

type Seg = { depth: number; len: number; angle: number; width: number; kids: Seg[] };

const BAYER = (() => {
  const m = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
  ];
  return m.map((row) => row.map((v) => (v + 0.5) / 64));
})();

function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) % 100000) / 100000;
  };
}

function grow(seed: number, maxDepth: number): Seg {
  const r = rng(seed);
  const make = (depth: number, len: number, angle: number, width: number): Seg => {
    const seg: Seg = { depth, len, angle, width, kids: [] };
    if (depth >= maxDepth) return seg;
    const n = depth < 2 ? 2 + Math.round(r()) : 2;
    for (let i = 0; i < n; i++) {
      const spread = 0.28 + r() * 0.3;
      const a = (i - (n - 1) / 2) * spread * 1.4 + (r() - 0.5) * 0.35;
      seg.kids.push(make(depth + 1, len * (0.68 + r() * 0.14), a, width * 0.68));
    }
    return seg;
  };
  return make(0, 1, 0, 1);
}

export function DitherTree({
  trees,
  ink = "#1f7a3a",
  paper = "transparent",
  cell = 4,
  light = 0.18,
  fade,
  className,
}: {
  trees: TreeSpec[];
  ink?: string;
  paper?: string;
  /** dot pitch in CSS px */
  cell?: number;
  /** strength of the travelling light, 0–1 */
  light?: number;
  /** soften a region so text stays readable: an ellipse centre and radii as fractions */
  fade?: { x: number; y: number; rx: number; ry: number };
  className?: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const visible = useVisible(host);
  const born = useRef<number | null>(null);

  useEffect(() => {
    const out = canvas.current;
    const octx = out?.getContext("2d");
    if (!out || !octx) return;
    const scene = document.createElement("canvas");
    const sctx = scene.getContext("2d", { willReadFrequently: true });
    if (!sctx) return;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const forests = trees.map((t) => ({ spec: t, root: grow(t.seed, t.depth ?? 8) }));
    let frame = 0;

    const draw = (now: number) => {
      const box = out.getBoundingClientRect();
      const dpr = Math.min(devicePixelRatio || 1, 2);
      const cols = Math.max(1, Math.floor(box.width / cell));
      const rows = Math.max(1, Math.floor(box.height / cell));
      if (scene.width !== cols || scene.height !== rows) {
        scene.width = cols;
        scene.height = rows;
      }
      if (out.width !== Math.round(box.width * dpr) || out.height !== Math.round(box.height * dpr)) {
        out.width = Math.round(box.width * dpr);
        out.height = Math.round(box.height * dpr);
      }
      if (born.current === null) born.current = now;
      const t = reduced ? 1e4 : (now - born.current) / 1000;
      const growth = reduced ? 1 : Math.min(1, t / 2.4);
      const ease = 1 - Math.pow(1 - growth, 3);

      // 1. greyscale scene: white = ink
      sctx.fillStyle = "#000";
      sctx.fillRect(0, 0, cols, rows);
      sctx.lineCap = "round";
      for (const { spec, root } of forests) {
        const base = rows * (spec.y ?? 1);
        const trunk = rows * spec.size;
        const branch = (seg: Seg, x: number, y: number, dir: number) => {
          const visibleUpTo = ease * 9;
          if (seg.depth > visibleUpTo) return;
          const part = Math.min(1, visibleUpTo - seg.depth);
          const sway = reduced ? 0 : Math.sin(t * 0.9 + seg.depth * 0.7 + spec.seed) * 0.035 * (seg.depth + 1);
          const a = dir + seg.angle + sway;
          const L = trunk * seg.len * (seg.depth === 0 ? 0.36 : 0.3) * part;
          const nx = x + Math.sin(a) * L;
          const ny = y - Math.cos(a) * L;
          const shade = 0.55 + 0.45 * (1 - seg.depth / 9);
          sctx.strokeStyle = `rgba(255,255,255,${shade})`;
          sctx.lineWidth = Math.max(0.7, trunk * 0.05 * seg.width);
          sctx.beginPath();
          sctx.moveTo(x, y);
          sctx.lineTo(nx, ny);
          sctx.stroke();
          if (seg.depth >= 3) {
            // canopy: soft blobs along the outer branches build up a crown
            const r = trunk * (0.05 + 0.012 * seg.depth) * part;
            const g = sctx.createRadialGradient(nx, ny, 0, nx, ny, r);
            g.addColorStop(0, `rgba(255,255,255,${seg.kids.length ? 0.22 : 0.42})`);
            g.addColorStop(1, "rgba(255,255,255,0)");
            sctx.fillStyle = g;
            sctx.beginPath();
            sctx.arc(nx, ny, r, 0, Math.PI * 2);
            sctx.fill();
          }
          for (const k of seg.kids) branch(k, nx, ny, a);
        };
        branch(root, cols * spec.x, base, spec.lean ?? 0);
      }

      // 2. a slow light sweeping across, so the dither shimmers
      if (light > 0) {
        const lx = ((Math.sin(t * 0.25) + 1) / 2) * cols;
        const g = sctx.createRadialGradient(lx, rows * 0.35, 0, lx, rows * 0.35, Math.max(cols, rows) * 0.6);
        g.addColorStop(0, `rgba(255,255,255,${light})`);
        g.addColorStop(1, "rgba(255,255,255,0)");
        sctx.globalCompositeOperation = "lighter";
        sctx.fillStyle = g;
        sctx.fillRect(0, 0, cols, rows);
        sctx.globalCompositeOperation = "source-over";
      }

      // 3. keep a reading area quiet
      if (fade) {
        const g = sctx.createRadialGradient(cols * fade.x, rows * fade.y, 0, cols * fade.x, rows * fade.y, 1);
        g.addColorStop(0, "rgba(0,0,0,1)");
        g.addColorStop(0.75, "rgba(0,0,0,0.85)");
        g.addColorStop(1, "rgba(0,0,0,0)");
        sctx.save();
        sctx.translate(cols * fade.x, rows * fade.y);
        sctx.scale(cols * fade.rx, rows * fade.ry);
        sctx.translate(-cols * fade.x, -rows * fade.y);
        sctx.fillStyle = g;
        sctx.beginPath();
        sctx.arc(cols * fade.x, rows * fade.y, 1, 0, Math.PI * 2);
        sctx.fill();
        sctx.restore();
      }

      // 4. print: Bayer threshold, one square dot per lit cell
      const px = sctx.getImageData(0, 0, cols, rows).data;
      octx.setTransform(1, 0, 0, 1, 0, 0);
      octx.clearRect(0, 0, out.width, out.height);
      if (paper !== "transparent") {
        octx.fillStyle = paper;
        octx.fillRect(0, 0, out.width, out.height);
      }
      const step = cell * dpr;
      const dot = Math.max(1, step - Math.max(1, dpr));
      octx.fillStyle = ink;
      octx.beginPath();
      for (let y = 0; y < rows; y++) {
        const by = BAYER[y & 7];
        for (let x = 0; x < cols; x++) {
          const v = px[(y * cols + x) * 4] / 255;
          if (v > by[x & 7]) octx.rect(x * step, y * step, dot, dot);
        }
      }
      octx.fill();

      if (!reduced && growth >= 0) frame = requestAnimationFrame(draw);
    };

    if (visible || reduced) frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [visible, trees, ink, paper, cell, light, fade]);

  return (
    <div ref={host} className={`dither-tree${className ? ` ${className}` : ""}`} aria-hidden="true">
      <canvas ref={canvas} />
    </div>
  );
}
