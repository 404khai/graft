import { useCallback, useEffect, useRef, type KeyboardEvent } from "react";

/** Nexvyn BounceSidebar, adapted from https://ui.nexvyn.dev/r/bounce-sidebar.json:
 * same listbox semantics, keyboard model and the dot that arcs outward as it
 * travels between items. Motion uses the Web Animations API instead of
 * `motion`; the optional sounds are omitted. */
export function BounceSidebar({
  items,
  value,
  onChange,
  className = "",
}: {
  items: string[];
  value: number;
  onChange: (index: number) => void;
  className?: string;
}) {
  const dot = useRef<HTMLSpanElement>(null);
  const itemRefs = useRef<(HTMLLIElement | null)[]>([]);
  const prevY = useRef<number | null>(null);

  const targetY = (index: number) => {
    const el = itemRefs.current[index];
    return el ? el.offsetTop + el.offsetHeight / 2 - 3 : null;
  };

  useEffect(() => {
    const element = dot.current;
    const toY = targetY(value);
    if (!element || toY === null) return;
    const fromY = prevY.current;
    prevY.current = toY;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (fromY === null || reduced || fromY === toY) {
      element.style.transform = `translate(0px, ${toY}px)`;
      return;
    }
    const distance = Math.abs(toY - fromY);
    const peakX = -Math.min(0.6, 20 / distance) * distance;
    element.animate(
      [
        { transform: `translate(0px, ${fromY}px)` },
        { transform: `translate(${peakX}px, ${fromY + (toY - fromY) * 0.62}px)`, offset: 0.4 },
        { transform: `translate(0px, ${toY}px)` },
      ],
      { duration: 450, easing: "cubic-bezier(0.22, 1, 0.36, 1)" },
    );
    element.style.transform = `translate(0px, ${toY}px)`;
  }, [value]);

  useEffect(() => {
    const snap = () => {
      const toY = targetY(value);
      if (toY !== null && dot.current) {
        dot.current.style.transform = `translate(0px, ${toY}px)`;
        prevY.current = toY;
      }
    };
    document.fonts?.ready.then(snap);
    window.addEventListener("resize", snap);
    return () => window.removeEventListener("resize", snap);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLUListElement>) => {
      const count = items.length;
      let next = value;
      if (event.key === "ArrowDown") next = (value + 1) % count;
      else if (event.key === "ArrowUp") next = (value - 1 + count) % count;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = count - 1;
      else return;
      event.preventDefault();
      onChange(next);
      itemRefs.current[next]?.querySelector("button")?.focus();
    },
    [items.length, value, onChange],
  );

  return (
    <ul role="listbox" aria-label="Docs" onKeyDown={onKeyDown} className={`bounce-sidebar ${className}`}>
      <span ref={dot} className="bounce-dot" aria-hidden="true" />
      {items.map((item, index) => (
        <li
          key={item}
          ref={(el) => {
            itemRefs.current[index] = el;
          }}
        >
          <button
            type="button"
            role="option"
            aria-selected={index === value}
            tabIndex={index === value ? 0 : -1}
            onClick={() => onChange(index)}
          >
            {item}
          </button>
        </li>
      ))}
    </ul>
  );
}
