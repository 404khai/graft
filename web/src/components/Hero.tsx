import { Flora } from "./Flora";

export function Hero() {
  return (
    <section className="hero">
      <div className="hero-flora" aria-hidden="true">
        <Flora kind="tree" className="hf-tree" />
        <Flora kind="leaf3" className="hf-leaf-a" style={{ "--r": "-24deg" }} />
        <Flora kind="leaf1" className="hf-leaf-b" style={{ "--r": "152deg", "--d": "-2s" }} />
        <Flora kind="leaf2" className="hf-leaf-c" style={{ "--r": "200deg", "--d": "-4s" }} />
        <Flora kind="branch" className="hf-branch" style={{ "--r": "-62deg", "--d": "-1s" }} />
      </div>
      <span className="pill"><i />0.1.0 — now on Linux &amp; macOS</span>
      <h1>
        One root. Every agent its own <em>graft.</em>
      </h1>
      <p className="lede">
        Copy‑on‑write workspaces for parallel coding agents. Each graft is a view of one shared
        Root plus a private delta, so it costs only what the agent changes.
      </p>
      <div className="actions">
        <a className="btn btn-green" href="/docs.html#quickstart">Get started →</a>
        <a className="btn btn-dark" href="#install"><span className="prompt">$</span> brew install graft</a>
      </div>
    </section>
  );
}
