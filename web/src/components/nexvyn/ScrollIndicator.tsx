import { useEffect, useRef, useState } from "react";

/** Nexvyn ScrollIndicator, adapted from https://ui.nexvyn.dev/r/scroll-indicator.json:
 * a 60-tick ruler with section marks, a sliding accent marker and "n/total"
 * readout, and section labels that fan in on hover. Tracks window scroll over
 * elements with `data-section-index`; springs become CSS transitions. */
export function ScrollIndicator({ sections }: { sections: { id: string; title: string }[] }) {
  const track = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const [active, setActive] = useState(0);
  const [hovered, setHovered] = useState(false);
  const ticks = 60;
  const last = ticks - 1;
  const tickY = (tick: number) => (tick / last) * height;
  const sectionTick = (index: number) => Math.round((index / Math.max(sections.length - 1, 1)) * last);

  useEffect(() => {
    const measure = () => setHeight(track.current?.clientHeight ?? 0);
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  useEffect(() => {
    const onScroll = () => {
      const marks = [...document.querySelectorAll<HTMLElement>("[data-section-index]")];
      let current = 0;
      marks.forEach((mark, index) => {
        if (mark.getBoundingClientRect().top < window.innerHeight * 0.35) current = index;
      });
      setActive(current);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, [sections]);

  const go = (index: number) => {
    const target = document.querySelector(`[data-section-index="${index}"]`);
    if (!target) return;
    const top = target.getBoundingClientRect().top + window.scrollY - 96;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    window.scrollTo({ top, behavior: reduced ? "auto" : "smooth" });
  };

  const activeTick = sectionTick(active);
  return (
    <div
      ref={track}
      className={`scroll-indicator ${hovered ? "hovered" : ""}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {Array.from({ length: ticks }, (_, tick) => (
        <i
          key={tick}
          className={`si-tick ${tick % 5 === 0 ? "major" : ""} ${tick <= activeTick ? "past" : ""}`}
          style={{ top: tickY(tick) }}
        />
      ))}
      {sections.map((section, index) => (
        <div key={section.id}>
          <i className={`si-mark ${index === active ? "active" : ""}`} style={{ top: tickY(sectionTick(index)) }} />
          <button
            type="button"
            className={`si-label ${index === active ? "active" : ""}`}
            style={{ top: tickY(sectionTick(index)) - 7, transitionDelay: hovered ? `${index * 35}ms` : "0ms" }}
            onClick={() => go(index)}
          >
            {section.title}
          </button>
        </div>
      ))}
      <div className="si-marker" style={{ transform: `translateY(${tickY(activeTick)}px)` }}>
        <i />
        <span>
          {sections.length ? `${active + 1}/${sections.length}` : "0/0"}
        </span>
      </div>
    </div>
  );
}
