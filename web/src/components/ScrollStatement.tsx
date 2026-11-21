import { useEffect, useRef, useState } from "react";
import { useVisible } from "./art/useVisible";
import { figures } from "../generated/figures";
import "../generated/anatomy.css";

const HOOK = "Stop duplicating your repo for every agent.";
// Grey to ink: #b4b8b3 -> #0e120f.
const FROM = [0xb4, 0xb8, 0xb3];
const TO = [0x0e, 0x12, 0x0f];
const mix = (t: number) => `rgb(${FROM.map((f, i) => Math.round(f + (TO[i] - f) * t)).join(", ")})`;
const WORDS = HOOK.split(" ");

/**
 * The page pins while you scroll through this section, and the hook fills in
 * word by word from grey to ink. Then Root and delta are explained. Without
 * JavaScript, or with reduced motion, the hook is simply shown in ink.
 */
export function ScrollStatement() {
  const pin = useRef<HTMLDivElement>(null);
  // 1 = fully inked: what the prerendered HTML shows before the client takes over.
  const [progress, setProgress] = useState(1);
  const figure = useRef<HTMLDivElement>(null);
  const figureOn = useVisible(figure, true);

  useEffect(() => {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const update = () => {
      const el = pin.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const travel = r.height - window.innerHeight;
      // Finish inking a little before the section unpins.
      setProgress(travel > 0 ? Math.min(1, Math.max(0, -r.top / (travel * 0.85))) : 1);
    };
    update();
    // Seven words: cheap enough to update straight from the scroll event.
    window.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, []);

  return (
    <section className="statement-sec" aria-labelledby="hook">
      <div ref={pin} className="hook-pin">
        <div className="hook-sticky">
          <span className="eyebrow">Why Graft</span>
          <h2 id="hook" className="hook">
            {WORDS.map((w, i) => {
              // Each word inks over its own slice of the scroll.
              const t = Math.min(1, Math.max(0, progress * WORDS.length - i));
              return (
                <span key={i} style={{ color: mix(t) }}>
                  {w}{i < WORDS.length - 1 ? " " : ""}
                </span>
              );
            })}
          </h2>
          <p className="hook-aside">
            Each graft is a view of one shared <b>Root</b> plus a private <b>delta</b>, so it costs
            only what the agent changes.
          </p>
        </div>
      </div>

      <div className="root-delta">
        <div className="rd-copy">
          <span className="eyebrow">In plain terms</span>
          <p className="rd-lede">
            Graft gives every AI coding agent its own copy of your project, without copying it.
          </p>
          <ol className="rd-plain">
            <li><span><b>Each agent gets its own folder</b> that looks and works like a normal git checkout. Virtual grafts appear in milliseconds, however big the repo is.</span></li>
            <li><span><b>The folder only stores what that agent changes.</b> Everything else is read from one shared copy, so ten agents don't cost ten repos of disk.</span></li>
            <li><span><b>Dependencies are installed once</b> and shared, so the second agent doesn't wait for <code>npm install</code>.</span></li>
            <li><span><b>When an agent is done</b>, Graft tests its work and lands it as an ordinary git commit.</span></li>
          </ol>
          <div className="rd-terms">
            <div>
              <span className="eyebrow">Root</span>
              <h3>One repository, shared.</h3>
              <p>
                Git's object store and a cache of built layers such as <code>node_modules</code>.
                Read-only to every graft, so it exists once however many agents you run.
              </p>
            </div>
            <div>
              <span className="eyebrow">Delta</span>
              <h3>Only what this agent changed.</h3>
              <p>
                The files a graft has created, edited or deleted, journaled in an op log. A graft is
                Root with its delta laid over it, so it costs what the agent changes, not what the
                repo weighs.
              </p>
            </div>
          </div>
          <a className="rd-more" href="/blog/why-worktrees-break-down">Why worktrees break down, with the numbers</a>
        </div>
        <div
          ref={figure}
          className={`rd-figure iso${figureOn ? " is-on" : ""}`}
          data-theme="light"
          dangerouslySetInnerHTML={{ __html: figures.lazy }}
        />
      </div>
    </section>
  );
}
