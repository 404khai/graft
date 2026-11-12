import { useEffect, useState } from "react";
import { GraftLogo } from "../GraftLogo";

/**
 * Claude and Codex wired to Graft: two clients either side of the mascot,
 * joined by lines with requests flowing in and responses flowing out.
 */
const prefersReduced = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function Wire({ flip }: { flip?: boolean }) {
  const [still] = useState(prefersReduced);
  const d = "M0 30 C40 30 60 30 100 30";
  return (
    <svg className={`mcp-wire${flip ? " flip" : ""}`} viewBox="0 0 100 60" preserveAspectRatio="none" aria-hidden="true">
      <path d={d} className="mcp-wire-base" />
      <path d={d} className="mcp-wire-flow" />
      {!still && (
        <>
          <circle r="3.2" className="mcp-pulse">
            <animateMotion dur="2.2s" repeatCount="indefinite" path={d} />
          </circle>
          <circle r="2.4" className="mcp-pulse back">
            <animateMotion dur="2.2s" begin="1.1s" repeatCount="indefinite" path={d} keyPoints="1;0" keyTimes="0;1" calcMode="linear" />
          </circle>
        </>
      )}
    </svg>
  );
}

export function McpLink({ className }: { className?: string }) {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (prefersReduced()) return;
    const id = window.setInterval(() => setTick((t) => t + 1), 1100);
    return () => window.clearInterval(id);
  }, []);
  return (
    <div className={`mcp-link${className ? ` ${className}` : ""}`}>
      <div className={`mcp-client${tick % 2 === 0 ? " ping" : ""}`}>
        <img src="/claude.svg" alt="Claude" />
      </div>
      <Wire />
      <GraftLogo className="mcp-logo" title="Graft" />
      <Wire flip />
      <div className={`mcp-client${tick % 2 === 1 ? " ping" : ""}`}>
        <img src="/codex.svg" alt="Codex" />
      </div>
    </div>
  );
}
