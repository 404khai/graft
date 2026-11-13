import type { ReactNode } from "react";

/**
 * Poster ground after Astrid's explainer poster: a deep field with offset
 * colour blocks, halftone dots, coordinate tags and grain. Content sits on
 * top. `layout` picks a block arrangement.
 */
export function Poster({
  layout = "card",
  className,
  children,
}: {
  layout?: "card" | "footer";
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`poster poster-${layout}${className ? ` ${className}` : ""}`}>
      <div className="poster-blocks" aria-hidden="true">
        <i className="pb pb1" />
        <i className="pb pb2" />
        <i className="pb pb3" />
        <i className="pb pb4" />
        <i className="pb pb5" />
        <i className="pb pb6" />
        <span className="halftone h1" />
        <span className="halftone h2" />
        <span className="coord c1"><b>G01</b><em>ROOT</em><em>0ff6509</em></span>
        <span className="coord c2"><b>FIG</b><em>Δ</em><em>04.4,03.1</em></span>
      </div>
      <div className="poster-content">{children}</div>
    </div>
  );
}
