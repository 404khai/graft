import type { CSSProperties } from "react";

/** The illustrations in public/, as decoration that peeks in from an edge. */
export type FloraKind = "tree" | "branch" | "leaf1" | "leaf2" | "leaf3" | "graft";

export function Flora({
  kind,
  className,
  style,
  sway = true,
}: {
  kind: FloraKind;
  className?: string;
  /** Position, size and rotation (`--r`) for this piece. */
  style?: CSSProperties & { "--r"?: string; "--d"?: string };
  sway?: boolean;
}) {
  return (
    <img
      className={`flora${sway ? " flora-sway" : ""}${className ? ` ${className}` : ""}`}
      src={`/${kind}.svg`}
      alt=""
      aria-hidden="true"
      draggable={false}
      style={style}
    />
  );
}
