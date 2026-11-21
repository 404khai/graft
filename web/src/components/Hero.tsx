import { Icon } from "./Icon";
import { InstallBox } from "./InstallBox";
import { DitherTree, type TreeSpec } from "./art/DitherTree";

const HERO_TREES: TreeSpec[] = [
  { x: 0.02, size: 0.72, lean: 0.14, seed: 7, depth: 9 },
  { x: 0.98, size: 0.66, lean: -0.12, seed: 21, depth: 9 },
  { x: 0.16, size: 0.32, lean: -0.04, seed: 13, depth: 7 },
  { x: 0.86, size: 0.28, lean: 0.06, seed: 3, depth: 7 },
];
const HERO_FADE = { x: 0.5, y: 0.52, rx: 0.45, ry: 0.62 };

export function Hero() {
  return (
    <section className="hero">
      <DitherTree
        className="hero-dither"
        trees={HERO_TREES}
        fade={HERO_FADE}
        light={0.05}
        ink="#6fa57e"
      />
      <span className="pill">
        <i aria-hidden="true" />
        0.2.0 — now on Linux &amp; macOS
      </span>
      <h1>
        One root. Every agent its own <em>graft.</em>
      </h1>
      <p className="lede">
        Copy‑on‑write workspaces for parallel coding agents. Each graft is a
        view of one shared Root plus a private delta, so it costs only what the
        agent changes.
      </p>
      <div className="actions">
        <InstallBox />
        <a className="btn btn-green" href="/docs/quickstart">
          Get started <Icon name="arrow-right" />
        </a>
      </div>
    </section>
  );
}
