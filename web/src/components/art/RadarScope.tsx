import { useEffect, useRef } from "react";
import { useVisible } from "./useVisible";

/**
 * A radar scope after React Bits' Radar background
 * (https://reactbits.dev/backgrounds/radar), drawn on a 2D canvas instead of
 * WebGL: rings, spokes, a rotating sweep with a fading trail and blips that
 * flare as the beam passes. Children (the leaf) sit at the centre. Pauses off
 * screen and holds still under prefers-reduced-motion.
 */
type Blip = { a: number; r: number; hit: number };

export function RadarScope({
  className,
  color = "184, 242, 140", // sprout, as "r, g, b"
  speed = 0.9, // radians per second
  rings = 4,
  spokes = 12,
  children,
}: {
  className?: string;
  color?: string;
  speed?: number;
  rings?: number;
  spokes?: number;
  children?: React.ReactNode;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const visible = useVisible(host);

  useEffect(() => {
    const cv = canvas.current;
    const ctx = cv?.getContext("2d");
    if (!cv || !ctx) return;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const blips: Blip[] = [
      { a: 0.7, r: 0.62, hit: -9 },
      { a: 2.4, r: 0.38, hit: -9 },
      { a: 3.9, r: 0.8, hit: -9 },
      { a: 5.2, r: 0.5, hit: -9 },
    ];
    let angle = -Math.PI / 2;
    let last = performance.now();
    let frame = 0;

    const draw = (now: number) => {
      const dpr = Math.min(devicePixelRatio || 1, 2);
      const { width: w, height: h } = cv.getBoundingClientRect();
      if (cv.width !== Math.round(w * dpr)) {
        cv.width = Math.round(w * dpr);
        cv.height = Math.round(h * dpr);
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      if (!reduced) angle = (angle + dt * speed) % (Math.PI * 2);
      const t = now / 1000;
      const cx = w / 2;
      const cy = h / 2;
      const R = Math.hypot(w, h) / 2;
      const ringGap = Math.min(w, h) / 2 / rings;
      const c = (a: number) => `rgba(${color}, ${a})`;

      ctx.clearRect(0, 0, w, h);

      // rings and spokes
      ctx.lineWidth = 1;
      for (let i = 1; i <= rings + 2; i++) {
        ctx.strokeStyle = c(i <= rings ? 0.22 : 0.1);
        ctx.beginPath();
        ctx.arc(cx, cy, i * ringGap, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.strokeStyle = c(0.1);
      for (let i = 0; i < spokes; i++) {
        const a = (i / spokes) * Math.PI * 2;
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(cx + Math.cos(a) * R, cy + Math.sin(a) * R);
        ctx.stroke();
      }

      // sweep: a conic trail behind the beam
      const trail = 1.1;
      const grad = ctx.createConicGradient(angle - trail, cx, cy);
      grad.addColorStop(0, c(0));
      grad.addColorStop(trail / (Math.PI * 2), c(0.32));
      grad.addColorStop(trail / (Math.PI * 2) + 0.0001, c(0));
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, R, angle - trail, angle);
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = c(0.85);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(cx + Math.cos(angle) * R, cy + Math.sin(angle) * R);
      ctx.stroke();

      // blips flare when the beam crosses them, then fade
      const maxR = Math.min(w, h) / 2;
      for (const b of blips) {
        const behind = ((angle - b.a) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2);
        if (behind < dt * speed * 1.5 || (reduced && b.hit < 0)) b.hit = t;
        const age = reduced ? 0.4 : t - b.hit;
        const life = Math.max(0, 1 - age / 2.6);
        if (life <= 0) continue;
        const x = cx + Math.cos(b.a) * b.r * maxR;
        const y = cy + Math.sin(b.a) * b.r * maxR;
        ctx.fillStyle = c(0.25 * life);
        ctx.beginPath();
        ctx.arc(x, y, 4 + (1 - life) * 14, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = c(0.95 * life);
        ctx.beginPath();
        ctx.arc(x, y, 3.5, 0, Math.PI * 2);
        ctx.fill();
      }

      if (!reduced) frame = requestAnimationFrame(draw);
    };

    if (visible || reduced) frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [visible, color, speed, rings, spokes]);

  return (
    <div ref={host} className={`radar-scope${className ? ` ${className}` : ""}`}>
      <canvas ref={canvas} aria-hidden="true" />
      {children && <div className="radar-scope-center">{children}</div>}
    </div>
  );
}
