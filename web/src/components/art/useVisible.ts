import { useEffect, useState, type RefObject } from "react";

/** True while the element is on screen; `once` keeps it true after the first sighting. */
export function useVisible(ref: RefObject<Element | null>, once = false) {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(([entry]) => {
      setVisible(entry.isIntersecting);
      if (entry.isIntersecting && once) io.disconnect();
    });
    io.observe(el);
    return () => io.disconnect();
  }, [ref, once]);
  return visible;
}
