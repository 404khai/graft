import { useEffect, useId, useRef, useState } from "react";

/**
 * The Graft mascot (public/graft.svg) with live eyes: the pupils wander and
 * follow the pointer, the eyes blink, and every few seconds the face switches
 * expression. Expressions are lids in the background colour, clipped to each
 * eye, after the reference cartoon-eye sheet.
 */

type Lid = { y: number; rot: number }; // top-lid edge offset from eye centre, and tilt in degrees
type Expression = {
  name: string;
  top: [Lid | null, Lid | null]; // left eye, right eye
  bottom?: [number | null, number | null]; // bottom-lid edge offset from centre
  brow: [number, number]; // extra brow lift in px (negative = up)
};

const EXPRESSIONS: Expression[] = [
  { name: "neutral", top: [null, null], brow: [0, 0] },
  { name: "sleepy", top: [{ y: -8, rot: 0 }, { y: -8, rot: 0 }], brow: [18, 18] },
  { name: "skeptical", top: [{ y: -10, rot: 0 }, null], brow: [22, -26] },
  { name: "angry", top: [{ y: -38, rot: 18 }, { y: -38, rot: -18 }], brow: [30, 30] },
  { name: "sad", top: [{ y: -42, rot: -16 }, { y: -42, rot: 16 }], brow: [-14, -14] },
  { name: "suspicious", top: [{ y: -30, rot: -4 }, { y: -30, rot: 4 }], bottom: [44, 44], brow: [16, 16] },
  { name: "surprised", top: [null, null], brow: [-34, -34] },
];

const EYES = [
  { cx: 266.4, cy: 415.2 },
  { cx: 505.3, cy: 415.2 },
];
const R = 101.9;
const BG = "#B8F28C";
const reduceMotion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export const EXPRESSION_NAMES = EXPRESSIONS.map((x) => x.name);

export function GraftLogo({
  className,
  title = "Graft",
  expression,
}: {
  className?: string;
  title?: string;
  /** Pin one expression (and stop cycling), e.g. for docs or tests. */
  expression?: string;
}) {
  const uid = useId().replace(/:/g, "");
  const svgRef = useRef<SVGSVGElement>(null);
  const [look, setLook] = useState({ x: 0, y: 0 });
  const [blink, setBlink] = useState(false);
  const [expr, setExpr] = useState(0);
  const lastPointer = useRef(0);

  // Follow the pointer; wander when it has been still for a while.
  useEffect(() => {
    if (reduceMotion()) return;
    const onMove = (e: PointerEvent) => {
      const box = svgRef.current?.getBoundingClientRect();
      if (!box) return;
      const dx = e.clientX - (box.left + box.width * 0.32);
      const dy = e.clientY - (box.top + box.height * 0.4);
      const d = Math.hypot(dx, dy) || 1;
      const reach = Math.min(1, d / 400);
      setLook({ x: (dx / d) * reach, y: (dy / d) * reach });
      lastPointer.current = performance.now();
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    const wander = window.setInterval(() => {
      if (performance.now() - lastPointer.current < 2500) return;
      const a = Math.random() * Math.PI * 2;
      const m = Math.random() < 0.25 ? 0 : 0.5 + Math.random() * 0.5;
      setLook({ x: Math.cos(a) * m, y: Math.sin(a) * m });
    }, 1400);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.clearInterval(wander);
    };
  }, []);

  // Blink at irregular intervals, sometimes twice.
  useEffect(() => {
    if (reduceMotion()) return;
    let t: number;
    const schedule = () => {
      t = window.setTimeout(() => {
        setBlink(true);
        window.setTimeout(() => setBlink(false), 130);
        if (Math.random() < 0.2) {
          window.setTimeout(() => setBlink(true), 260);
          window.setTimeout(() => setBlink(false), 390);
        }
        schedule();
      }, 2200 + Math.random() * 3200);
    };
    schedule();
    return () => window.clearTimeout(t);
  }, []);

  // Cycle expressions, returning to neutral between them.
  useEffect(() => {
    if (expression !== undefined || reduceMotion()) return;
    let t: number;
    const next = (current: number) => {
      t = window.setTimeout(() => {
        const n = current === 0 ? 1 + Math.floor(Math.random() * (EXPRESSIONS.length - 1)) : 0;
        setExpr(n);
        next(n);
      }, current === 0 ? 3000 + Math.random() * 3000 : 1800 + Math.random() * 1200);
    };
    next(0);
    return () => window.clearTimeout(t);
  }, [expression]);

  const pinned = EXPRESSIONS.find((x) => x.name === expression);
  const e = pinned ?? EXPRESSIONS[expr];
  const px = look.x * 40;
  const py = look.y * 34;

  return (
    <svg
      ref={svgRef}
      className={`graft-logo${className ? ` ${className}` : ""}`}
      viewBox="0 0 1199 1026"
      role="img"
      aria-label={title}
      data-expression={e.name}
    >
      <defs dangerouslySetInnerHTML={{ __html: filterDefs(uid) }} />
      <defs>
        {EYES.map((eye, i) => (
          <clipPath key={i} id={`${uid}-eye${i}`}>
            <circle cx={eye.cx} cy={eye.cy} r={R} />
          </clipPath>
        ))}
      </defs>

      <rect width="1024" height="1024" fill={BG} />
      <g filter={`url(#${uid}-f0)`} fill="#FF975B">
        <rect x="852.537" y="626.333" width="46" height="395" transform="rotate(-1.90879 852.537 626.333)" />
        <rect x="955.346" y="659.927" width="46" height="109" transform="rotate(49.8693 955.346 659.927)" />
        <rect x="777.119" y="812.664" width="46" height="134.588" transform="rotate(-39.4232 777.119 812.664)" />
        <rect x="962.422" y="884.816" width="46" height="109" transform="rotate(53.9495 962.422 884.816)" />
      </g>

      {/* brows */}
      <g className="gl-brow" style={{ transform: `translateY(${e.brow[0] * 1.7}px)` }}>
        <rect width="170.568" height="77.2714" transform="matrix(0.914232 -0.405191 0.356919 0.934135 137.216 204.188)" fill="#FF975B" />
      </g>
      <g className="gl-brow" style={{ transform: `translateY(${e.brow[1] * 1.7}px)` }}>
        <rect x="403.216" y="190.076" width="169" height="78" fill="#FF975B" />
      </g>

      {/* eyes */}
      {EYES.map((eye, i) => {
        const top = e.top[i];
        const bottom = e.bottom?.[i] ?? null;
        return (
          <g key={i} className={`gl-eye${blink ? " is-blink" : ""}`} style={{ transformOrigin: `${eye.cx}px ${eye.cy}px` }}>
            <g clipPath={`url(#${uid}-eye${i})`}>
              <circle cx={eye.cx} cy={eye.cy} r={R} fill="white" />
              <ellipse
                className="gl-pupil"
                cx={eye.cx + 0.4}
                cy={eye.cy - 20.5}
                rx="49.87"
                ry="57.19"
                fill="black"
                style={{ transform: `translate(${px}px, ${py}px)` }}
              />
              {/* top lid: a block whose lower edge sits at eye.cy + y, tilted */}
              <rect
                className="gl-lid"
                x={eye.cx - 160}
                y={eye.cy - 330}
                width="320"
                height="220"
                fill={BG}
                style={{
                  transformOrigin: `${eye.cx}px ${eye.cy}px`,
                  transform: top ? `translateY(${top.y + 110}px) rotate(${top.rot}deg)` : "translateY(0px) rotate(0deg)",
                }}
              />
              <rect
                className="gl-lid"
                x={eye.cx - 160}
                y={eye.cy + 110}
                width="320"
                height="220"
                fill={BG}
                style={{ transform: `translateY(${bottom !== null ? bottom - 110 : 0}px)` }}
              />
            </g>
            {/* covers the anti-aliased white fringe where a lid meets the eye edge */}
            <circle cx={eye.cx} cy={eye.cy} r={R + 1.5} fill="none" stroke={BG} strokeWidth="4" />
          </g>
        );
      })}

      <g filter={`url(#${uid}-f1)`}>
        <path d="M872.751 427.768L1086.9 712.373L984.743 695.129L981.409 794.955L890.078 728.784L821.177 826.762L790.407 721.828L684.969 793.275L708.692 743.908L872.751 427.768Z" fill="#1F7A3A" />
      </g>
      <g filter={`url(#${uid}-f2)`}>
        <path d="M802.638 265.471L1034.07 430.611L952.167 442.049L973.632 518.835L888.203 490.427L859.317 581.571L810.666 509.098L747.588 588.832L753.775 545.552L802.638 265.471Z" fill="#003A11" />
      </g>
      <g filter={`url(#${uid}-f3)`}>
        <path d="M775.436 162.351L992.328 302.152L919.483 316.139L902.717 382.916L849.414 357.944L843.99 446.926L796.282 383.248L744.153 458.43L747.281 418.853L775.436 162.351Z" fill="#59A02C" />
      </g>
      <path d="M491 603L525.252 736.417L454.258 714.147L407.19 762.542L372.263 716.6L324.46 788.995L313.18 713.074L230.867 752.019L252.758 720.154L313.18 553.5L491 603Z" fill="black" />
    </svg>
  );
}

// Rough-edge and grain filters from public/graft.svg, condensed, with ids scoped per instance.
function filterDefs(uid: string) {
  const rough = (seed: number) =>
    `<feTurbulence type="fractalNoise" baseFrequency="0.25" numOctaves="3" seed="${seed}"/>` +
    `<feDisplacementMap in="SourceGraphic" scale="8" xChannelSelector="R" yChannelSelector="G" result="rough"/>`;
  const grain = (seed: number) =>
    `<feTurbulence type="fractalNoise" baseFrequency="2" stitchTiles="stitch" numOctaves="3" seed="${seed}" result="noise"/>` +
    `<feColorMatrix in="noise" type="luminanceToAlpha" result="a"/>` +
    `<feComponentTransfer in="a" result="dots"><feFuncA type="discrete" tableValues="1 1 1 1 1 0 0 0 0 0"/></feComponentTransfer>` +
    `<feComposite operator="in" in="dots" in2="rough" result="clip"/>` +
    `<feFlood flood-color="rgba(0,0,0,0.25)"/><feComposite operator="in" in2="clip" result="shade"/>` +
    `<feMerge><feMergeNode in="rough"/><feMergeNode in="shade"/></feMerge>`;
  const box = 'filterUnits="userSpaceOnUse" x="0" y="0" width="1199" height="1026" color-interpolation-filters="sRGB"';
  return (
    `<filter id="${uid}-f0" ${box}>${rough(5017)}</filter>` +
    `<filter id="${uid}-f1" ${box}>${rough(4751)}${grain(7406)}</filter>` +
    `<filter id="${uid}-f2" ${box}>${rough(3482)}${grain(1478)}</filter>` +
    `<filter id="${uid}-f3" ${box}>${rough(5902)}${grain(5282)}</filter>`
  );
}
