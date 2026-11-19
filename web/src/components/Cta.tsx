import { DitherTree, type TreeSpec } from "./art/DitherTree";
import { InstallBox } from "./InstallBox";

// One tree rooted at the left edge, leaning in so its branches reach toward the middle.
const CTA_TREES: TreeSpec[] = [{ x: 0.0, size: 1.2, lean: 0.62, seed: 7, depth: 10 }];
const CTA_FADE = { x: 0.55, y: 0.5, rx: 0.4, ry: 0.5 };

export function Cta() {
  return (
    <section className="cta" id="install">
      <DitherTree className="cta-dither" trees={CTA_TREES} fade={CTA_FADE} ink="#6fa57e" light={0.04} />
      <h2>Plant one Root.<br />Grow as many agents as you like.</h2>
      <InstallBox suffix=" && graft init" />
    </section>
  );
}
