const overlaps = [
  { sev: "behaviour", pair: "auth ↔ billing", detail: "speculative merge: tests fail together" },
  { sev: "symbol", pair: "auth ↔ api", detail: "src/session.rs · fn refresh_token" },
  { sev: "path", pair: "api ↔ search", detail: "src/routes.ts · different functions" },
];

const steps = ["rebase onto Root", "structural merge · Mergiraf", "test gate", "plain git commit →"];

const small = [
  { label: "Blueprints", title: "A private runtime per graft.", body: "Ports, Postgres databases, Docker volumes and .env, declared once in graft.toml." },
  { label: "Bud & Ring", title: "Fork, checkpoint, rewind.", body: "Try two approaches at once. Back out of a bad path without losing the good one." },
  { label: "graft run", title: "Confined to its own graft.", body: "Landlock on Linux, Seatbelt on macOS. Isolation is a guarantee, not a prompt.", className: "cell-run" },
];

export function Features() {
  return (
    <section className="features" id="features">
      <div className="section-head">
        <div>
          <span className="eyebrow">[01] What a worktree leaves to you</span>
          <h2>Everything parallel agents need, grown from one Root.</h2>
        </div>
        <p>Cheap workspaces are the start. Graft adds runtime, overlap detection, a merge queue and enforcement.</p>
      </div>

      <div className="bento">
        <article className="cell cell-radar">
          <div className="cell-head">
            <span className="label">Radar</span>
            <h3>See collisions while agents work, not at merge time.</h3>
          </div>
          <ul className="overlaps">
            {overlaps.map((o) => (
              <li key={o.pair}>
                <span className={`sev sev-${o.sev}`}>● {o.sev}</span>
                <b>{o.pair}</b>
                <span>{o.detail}</span>
              </li>
            ))}
          </ul>
        </article>

        <article className="cell cell-integrate">
          <div className="cell-head">
            <span className="label">Integrate</span>
            <h3>A merge queue that tests before it lands.</h3>
          </div>
          <ol className="steps">
            {steps.map((s, i) => (
              <li key={s} className={i === steps.length - 1 ? "hl" : undefined}>
                <span>{String(i + 1).padStart(2, "0")}</span>{s}
              </li>
            ))}
          </ol>
        </article>

        {small.map((c) => (
          <article key={c.label} className={`cell ${c.className ?? ""}`}>
            <span className="label">{c.label}</span>
            <div>
              <h4>{c.title}</h4>
              <p>{c.body}</p>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
