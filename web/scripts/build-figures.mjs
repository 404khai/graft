// Builds the blog's isometric figures with the vendored Anatomy kit and
// writes them as SVG strings to src/generated/figures.ts, plus the kit's
// stylesheet to src/generated/anatomy.css. Run: node scripts/build-figures.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as k from "../vendor/anatomy/iso-kit.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "../src/generated");

const W = 640;
const H = 400;
const S = (paths, style) => k.solidSvg(paths, style);
const L = (d, style) => k.lineSvg(d, style);
const T = (at, z, P, text, opts) => k.faceTextSvg(k.topMatrix(at, z, P, "x"), text, opts);

/** A labelled slab: plan on the floor plane, from z to z + h. */
function slab(P, plan, z, h, { label, sub, tone = "hi", lit = false, size = 6 } = {}) {
  const out = [S(k.slabOf(plan, z, h, P, 8, 1.2), { tone, lit })];
  out.push(L(k.planOutline(k.insetPlan(plan, 4), z + h, P), { tone: "faint" }));
  if (label) out.push(T([plan.x + 8, plan.y + 14], z + h, P, label, { size, tone: "hi" }));
  if (sub) out.push(T([plan.x + 8, plan.y + 24], z + h, P, sub, { size: size * 0.66, tone: "mid" }));
  return out.join("");
}

/** Lazy reads: one Root object store, grafts as thin views above it, each with its own delta. */
function lazy() {
  const root = { x: -120, y: -80, w: 240, d: 160, r: 10 };
  const grafts = [
    { x: -112, y: -72, name: "auth" },
    { x: -40, y: -18, name: "api" },
    { x: 32, y: 36, name: "billing" },
  ];
  const P = k.fitProjection(
    [...k.boxCorners(root, -24, 0), [-110, -70, 120], [130, 110, 120]],
    W,
    H,
    { pad: 30 },
  );
  const out = [`<path class="iso-halo" d="${k.haloOf(root, -24, 24, P)}"/>`];
  out.push(slab(P, root, -24, 24, { label: "ROOT · GIT OBJECTS", sub: "shared, read-only", size: 10 }));
  // object dots on the root
  out.push(k.dotsSvg(k.dotGrid(-100, 20, 14, 4, 14, 0, P), { size: 0.45, tone: "lo" }));
  for (const [i, g] of grafts.entries()) {
    const z = 60 + i * 22;
    const plan = { x: g.x, y: g.y, w: 96, d: 64, r: 6 };
    // reads fall through to the root
    for (const [cx, cy] of k.corners(plan, 6)) out.push(L(k.segment([cx, cy, 0], [cx, cy, z], P), { tone: "faint", dotted: true }));
    out.push(slab(P, plan, z, 3, { label: g.name.toUpperCase(), sub: "graft", size: 9 }));
    // the private delta: a small block on top
    const delta = { x: g.x + 64, y: g.y + 36, w: 22, d: 18, r: 2 };
    out.push(S(k.slabOf(delta, z + 3, 10, P, 4), { tone: "hi", lit: true }));
  }
  out.push(T([-112, 70], 0, P, "Δ = WHAT EACH AGENT CHANGED", { size: 7, tone: "mid" }));
  return k.figureSvg({ width: W, height: H, label: "Grafts are thin views over Root's object store, each with a private delta", body: out });
}

/** The op log: a column of journal rows beside the delta they replay into. */
function oplog() {
  const rows = ["create  src/a.ts", "write   src/a.ts", "rename  b.ts → c.ts", "unlink  old.md", "write   README.md", "chmod   run.sh"];
  const P = k.fitProjection([[-150, -60, 0], [150, 70, 0], [-150, -60, 130], [150, 70, 60]], W, H, { pad: 30 });
  const out = [];
  rows.forEach((r, i) => {
    const plan = { x: -150, y: -46, w: 150, d: 70, r: 4 };
    const z = i * 18;
    out.push(S(k.slabOf(plan, z, 12, P, 6, 0.8), { tone: i === rows.length - 1 ? "hi" : "mid", lit: i === rows.length - 1 }));
    out.push(T([plan.x + 8, plan.y + 22], z + 12, P, `${String(i + 1).padStart(3, "0")}  ${r}`, { size: 8, tone: "hi" }));
  });
  out.push(T([-150, -58], rows.length * 18, P, "OP LOG · ONE TRANSACTION PER ROW", { size: 7, tone: "mid" }));
  const delta = { x: 40, y: -30, w: 110, d: 90, r: 8 };
  out.push(`<path class="iso-halo" d="${k.haloOf(delta, 0, 30, P)}"/>`);
  out.push(slab(P, delta, 0, 30, { label: "DELTA", sub: "replay(log) == delta", size: 10 }));
  out.push(k.wireSvg(k.iso([0, 0, 50], P), k.iso([40, 10, 30], P), { tone: "mid", dashed: true, flow: true, ends: "end" }));
  return k.figureSvg({ width: W, height: H, label: "Each write is an op log row; replaying the log rebuilds the delta exactly", body: out });
}

/** Cache tree: the index with and without git's TREE extension. */
function cachetree() {
  const P = k.fitProjection([[-160, -70, 0], [160, 80, 0], [-160, -70, 90]], W, H, { pad: 30 });
  const out = [];
  const left = { x: -160, y: -60, w: 130, d: 130, r: 8 };
  const right = { x: 30, y: -60, w: 130, d: 130, r: 8 };
  out.push(slab(P, left, 0, 16, { label: "INDEX, NO TREE", sub: "compare every entry", size: 9 }));
  out.push(slab(P, right, 0, 16, { label: "INDEX + TREE", sub: "skip unchanged dirs", size: 9 }));
  // every entry checked on the left
  out.push(k.dotsSvg(k.dotGrid(left.x + 12, left.y + 40, 10, 7, 11, 16, P), { size: 0.55, tone: "hi" }));
  // only the changed directory checked on the right
  out.push(k.dotsSvg(k.dotGrid(right.x + 12, right.y + 40, 10, 7, 11, 16, P), { size: 0.4, tone: "lo" }));
  out.push(S(k.slabOf({ x: right.x + 60, y: right.y + 70, w: 30, d: 30, r: 3 }, 16, 10, P, 4), { tone: "hi", lit: true }));
  out.push(T([left.x, left.y + left.d + 14], 0, P, "≈ 12 MS", { size: 14, tone: "hi" }));
  out.push(T([right.x, right.y + right.d + 14], 0, P, "≈ 0.5 MS", { size: 14, tone: "hi" }));
  return k.figureSvg({ width: W, height: H, label: "Without a cache tree git compares the whole index; with one it skips unchanged directories", body: out });
}

mkdirSync(OUT, { recursive: true });
const figures = { lazy: lazy(), oplog: oplog(), cachetree: cachetree() };
writeFileSync(
  join(OUT, "figures.ts"),
  `// Generated by scripts/build-figures.mjs. Do not edit.\nexport const figures = ${JSON.stringify(figures, null, 0)} as const;\nexport type FigureId = keyof typeof figures;\n`,
);
writeFileSync(join(OUT, "anatomy.css"), `/* Generated from the Anatomy kit's ISO_CSS. Do not edit. */\n${k.ISO_CSS}\n`);
console.log("wrote", Object.keys(figures).join(", "));
