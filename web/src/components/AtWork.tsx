import { Poster } from "./Poster";
import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";

type Result = {
  id: string;
  label: string;
  icon: ReactNode;
  cards: { label: string; text: string }[];
  statement: string;
  source: string;
};

// Numbers come from bench/results (run 42113580319, median of 3 runs).
const results: Result[] = [
  {
    id: "disk",
    label: "Disk",
    icon: <><rect x="2" y="10" width="4" height="8" /><rect x="8" y="6" width="4" height="12" /><rect x="14" y="2" width="4" height="16" /></>,
    cards: [
      { label: "macOS · APFS · 200k files", text: "4 grafts add 302 MiB. 4 worktrees add 5.5 GiB." },
      { label: "Linux · XFS · 1k files", text: "8 grafts add 6 MiB. 8 worktrees add 62 MiB." },
    ],
    statement: "A graft costs what the agent changes, not what the repo weighs. Unchanged files share disk blocks with Root.",
    source: "bench run 42113580319 · median of 3",
  },
  {
    id: "create",
    label: "Create & prune",
    icon: <path d="M11 2 4 11h5l-1 7 7-9h-5z" />,
    cards: [
      { label: "macOS · NFS backend · 200k files", text: "Create 4 grafts in 49 ms. 4 worktrees take 24 s." },
      { label: "macOS · NFS backend · 200k files", text: "Prune a graft in 55 ms. A worktree takes 12 s." },
    ],
    statement: "Virtual grafts write metadata, not files. Creating and pruning one doesn't grow with the repo.",
    source: "bench run 42113580319 · median of 3",
  },
  {
    id: "layers",
    label: "Shared layers",
    icon: <><rect x="3" y="3" width="6" height="6" /><rect x="11" y="3" width="6" height="6" /><rect x="3" y="11" width="6" height="6" /><rect x="11" y="11" width="6" height="6" /></>,
    cards: [
      { label: "Linux · XFS · 1k files", text: "Later agents are ready to test in 187 ms. A worktree needs 424 ms." },
      { label: "Layer cache", text: "node_modules is built once per lockfile, then shared by every graft." },
    ],
    statement: "Install once. Every graft after the first starts with its dependencies already in place.",
    source: "bench run 42113580319 · median of 3",
  },
  {
    id: "radar",
    label: "Radar",
    icon: <><circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" strokeWidth="2" /><circle cx="10" cy="10" r="2" /></>,
    cards: [
      { label: "Symbol level", text: "Two grafts edit the same function. Radar names both and the symbol." },
      { label: "Behaviour level", text: "Both pass alone, fail together. A speculative merge catches it." },
    ],
    statement: "Every write is journaled centrally, so overlaps surface while they're still cheap to fix.",
    source: "path · symbol · behaviour",
  },
];

const arrowStep: Record<string, number> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };

export function AtWork() {
  const [active, setActive] = useState(0);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  function select(i: number) {
    setActive(i);
    tabRefs.current[i]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  function onKeyDown(e: KeyboardEvent) {
    const step = arrowStep[e.key];
    if (!step) return;
    e.preventDefault();
    const next = (active + step + results.length) % results.length;
    tabRefs.current[next]?.focus();
    select(next);
  }

  const r = results[active];

  return (
    <section className="at-work" id="at-work">
      <div className="at-work-head">
        <h2>See Graft at work.</h2>
        <a className="btn btn-ghost-dark" href="#">Read the benchmarks ›</a>
      </div>

      <div className="at-work-body">
        <div className="tabs" role="tablist" aria-label="Results" onKeyDown={onKeyDown}>
          {results.map((t, i) => (
            <button
              key={t.id}
              ref={(el) => { tabRefs.current[i] = el; }}
              role="tab"
              id={`t-${t.id}`}
              aria-selected={i === active}
              aria-controls={`p-${t.id}`}
              tabIndex={i === active ? 0 : -1}
              onClick={() => select(i)}
            >
              <svg viewBox="0 0 20 20" aria-hidden="true">{t.icon}</svg>
              {t.label}
            </button>
          ))}
        </div>

        <div className="panel" role="tabpanel" id={`p-${r.id}`} aria-labelledby={`t-${r.id}`}>
          <div className="cards">
            {r.cards.map((c) => (
              <div className="card" key={c.text}>
                <span className="label">{c.label}</span>
                <p>{c.text}</p>
              </div>
            ))}
          </div>
          <Poster className="statement">
            <div className="statement-panel">
              <p>{r.statement}</p>
              <span>{r.source}</span>
            </div>
          </Poster>
        </div>
      </div>
    </section>
  );
}
