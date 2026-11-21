// The 1200×630 link-preview card. Not part of the site build: render it with
// `npm run og` (vite dev + headless Chrome) and commit public/og.png.
import { createRoot } from "react-dom/client";
import { DitherTree } from "../src/components/art/DitherTree";
import { GraftLogo } from "../src/components/GraftLogo";
import "@fontsource-variable/funnel-display/wght.css";
import "@fontsource-variable/geist-mono/wght.css";
import "../src/styles.css";
import "./og.css";

function Card() {
  return (
    <div className="og">
      <DitherTree
        className="og-dither"
        trees={[
          { x: 0.0, size: 1.15, lean: 0.6, seed: 7, depth: 10 },
          { x: 0.97, size: 0.7, lean: -0.14, seed: 21, depth: 9 },
        ]}
        fade={{ x: 0.56, y: 0.5, rx: 0.36, ry: 0.5 }}
        ink="#6fa57e"
        light={0}
        cell={5}
      />
      <div className="og-copy">
        <div className="og-brand">
          <GraftLogo className="og-logo" title="" expression="neutral" />
          <span>graft</span>
        </div>
        <h1>One root. Every agent its own <em>graft.</em></h1>
        <p>Copy‑on‑write workspaces for parallel coding agents. A graft costs only what the agent changes.</p>
        <div className="og-row">
          <span className="og-pill"><i />Linux &amp; macOS</span>
          <code>$ brew install 404khai/graft/graft</code>
        </div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("og")!).render(<Card />);
