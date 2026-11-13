import { useEffect, useId, useRef, useState, type PointerEvent } from "react";

/** Nexvyn NavigationCompass, adapted from https://ui.nexvyn.dev/r/navigation-compass.json:
 * the same dial geometry, spike artwork, 180 ticks that reach toward the active
 * zone, degree labels and links that light up as they pass it. The dial turns
 * with page scroll and can be dragged with inertia; springs are a small
 * per-frame integrator instead of `motion`. */
export type CompassLink = { angle: number; label: string; href: string };

const ROSE_R = 93.327 * 0.6;
const ROSE_SCALE = 0.9;
const SPIKE_FILLS = [
  "M0.499548 92.9137C27.3142 82.6937 63.9363 57.6958 110.366 17.9201L21.7657 107.614C15.6859 102.901 9.59766 96.8914 0.499548 92.9137Z",
  "M110.509 18.0609C71.3409 65.0046 46.8222 101.949 36.9522 128.895C34.0939 121.069 27.9954 113.762 21.909 107.754L110.509 18.0609Z",
];
const SPIKE_CONTOUR =
  "M110.366 17.9201C63.9363 57.6958 27.3142 82.6937 0.499548 92.9137C17.4057 101.593 30.8783 113.604 36.8096 128.754C46.6795 101.808 71.1983 64.8638 110.366 17.9201ZM110.366 17.9201L21.7657 107.614";
const SPIKE_INNER = { x: 21.7657, y: 107.614 };

const toRad = (deg: number) => ((deg - 90) * Math.PI) / 180;
const polar = (c: number, r: number, deg: number) => ({ x: c + Math.cos(toRad(deg)) * r, y: c + Math.sin(toRad(deg)) * r });
const angularDistance = (a: number, b: number) => {
  const diff = Math.abs(a - b) % 360;
  return diff > 180 ? 360 - diff : diff;
};
const proximity = (angle: number, rotation: number, zone: number, threshold: number) => {
  const absolute = (((angle + rotation) % 360) + 360) % 360;
  const distance = angularDistance(absolute, zone);
  if (distance >= threshold) return 0;
  const t = 1 - distance / threshold;
  return t * t * (3 - 2 * t);
};

export function NavigationCompass({
  links,
  activeZoneAngle = 345,
  threshold = 15,
  roseRotation = 30,
  size = 756,
  className = "",
}: {
  links: CompassLink[];
  activeZoneAngle?: number;
  threshold?: number;
  roseRotation?: number;
  size?: number;
  className?: string;
}) {
  const c = size / 2;
  const outer = Math.round(size * 0.4233);
  const major = Math.round(size * 0.3836);
  const medium = Math.round(size * 0.3968);
  const minor = Math.round(size * 0.4101);
  const textR = Math.round(size * 0.4762);
  const numberR = Math.round(size * 0.4497);
  const filter = useId();
  const host = useRef<HTMLDivElement>(null);
  const [rotation, setRotation] = useState(-15);
  const manual = useRef(0);
  const velocity = useRef(0);
  const drag = useRef<{ angle: number } | null>(null);

  useEffect(() => {
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) return;
    let frame = 0;
    let current = -15;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      if (!drag.current) {
        manual.current += velocity.current * dt;
        velocity.current *= Math.exp(-dt * 3);
      }
      const target = -15 + window.scrollY * 0.05 + manual.current;
      current += (target - current) * (1 - Math.exp(-dt * 6));
      setRotation(current);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  const angleAt = (event: PointerEvent) => {
    const box = host.current!.getBoundingClientRect();
    return (Math.atan2(event.clientY - (box.top + box.height / 2), event.clientX - (box.left + box.width / 2)) * 180) / Math.PI;
  };
  const onDown = (event: PointerEvent) => {
    (event.target as Element).setPointerCapture?.(event.pointerId);
    drag.current = { angle: angleAt(event) };
    velocity.current = 0;
  };
  const onMove = (event: PointerEvent) => {
    if (!drag.current) return;
    const angle = angleAt(event);
    let delta = angle - drag.current.angle;
    if (delta > 180) delta -= 360;
    if (delta < -180) delta += 360;
    manual.current += delta;
    velocity.current = delta * 60;
    drag.current.angle = angle;
  };
  const onUp = () => {
    drag.current = null;
  };

  return (
    <div ref={host} className={`nav-compass ${className}`}>
      <svg
        viewBox={`0 0 ${size} ${size}`}
        role="navigation"
        aria-label="Docs compass"
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
      >
        <defs>
          <filter id={filter} filterUnits="userSpaceOnUse" x="0" y="0" width={size} height={size}>
            <feOffset dx="0" dy="6" />
            <feGaussianBlur stdDeviation="12" result="blur" />
            <feComposite operator="out" in="SourceGraphic" in2="blur" result="inverse" />
            <feFlood floodColor="black" floodOpacity="0.35" result="color" />
            <feComposite operator="in" in="color" in2="inverse" result="shadow" />
            <feComposite operator="over" in="shadow" in2="SourceGraphic" />
          </filter>
        </defs>
        <circle cx={c} cy={c} r="254" className="nc-face" filter={`url(#${filter})`} />
        <circle cx={c} cy={c} r="250" className="nc-ring" />
        <circle cx={c} cy={c} r="242" className="nc-ring" strokeWidth="2" />
        <circle cx={c} cy={c} r="230" className="nc-ring faint" strokeDasharray="4 4" />
        <g className="nc-spikes" style={{ transformOrigin: `${c}px ${c}px` }}>
          <g transform={`translate(${c}, ${c}) scale(${ROSE_SCALE}) rotate(${roseRotation})`}>
            {[45, 135, 225, 315].map((bearing) => (
              <g key={bearing} transform={`rotate(${bearing})`}>
                <g transform={`translate(0, ${-ROSE_R}) rotate(-44.65) translate(${-SPIKE_INNER.x}, ${-SPIKE_INNER.y})`}>
                  {SPIKE_FILLS.map((d) => (
                    <path key={d} d={d} className="nc-spike" vectorEffect="non-scaling-stroke" />
                  ))}
                  <path d={SPIKE_CONTOUR} className="nc-spike-line" strokeDasharray="4.26 4.26" vectorEffect="non-scaling-stroke" />
                </g>
              </g>
            ))}
            <circle r={ROSE_R} className="nc-dial" vectorEffect="non-scaling-stroke" />
          </g>
        </g>
        <g style={{ transform: `rotate(${rotation}deg)`, transformOrigin: `${c}px ${c}px` }}>
          <circle cx={c} cy={c} r={outer} className="nc-ring" strokeDasharray="6 3" />
          {Array.from({ length: 180 }, (_, index) => {
            const deg = index * 2;
            const isMajor = deg % 15 === 0;
            const base = isMajor ? major : deg % 5 === 0 ? medium : minor;
            const r = base - proximity(deg, rotation, activeZoneAngle, threshold) * 25;
            const a = polar(c, r, deg);
            const b = polar(c, outer, deg);
            return <line key={index} x1={a.x} y1={a.y} x2={b.x} y2={b.y} className={`nc-tick ${isMajor ? "major" : ""}`} />;
          })}
          {links.map((link) => {
            const near = proximity(link.angle, rotation, activeZoneAngle, threshold);
            const at = polar(c, textR, link.angle);
            return (
              <a key={link.label} href={link.href}>
                <line {...(() => { const p1 = polar(c, 200, link.angle); const p2 = polar(c, 450, link.angle); return { x1: p1.x, y1: p1.y, x2: p2.x, y2: p2.y }; })()} stroke="transparent" strokeWidth="40" />
                <g transform={`translate(${at.x}, ${at.y}) rotate(${-rotation}) scale(${1 + near * 0.2})`}>
                  <text textAnchor="middle" dominantBaseline="middle" className={`nc-link ${near > 0 ? "near" : ""}`}>
                    {link.label}
                  </text>
                </g>
              </a>
            );
          })}
          {Array.from({ length: 24 }, (_, index) => {
            const angle = index * 15;
            if (links.some((link) => Math.abs(link.angle - angle) < 10)) return null;
            const at = polar(c, numberR, angle);
            return (
              <text key={angle} transform={`translate(${at.x}, ${at.y}) rotate(${angle})`} textAnchor="middle" dominantBaseline="middle" className="nc-degree">
                {angle}
              </text>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
