import { Icon } from "../components/Icon";
import { DitherTree, type TreeSpec } from "../components/art/DitherTree";
import { GraftLogo } from "../components/GraftLogo";
import type { DocSection } from "./content";

const SWATCHES = [
  ["Ground", "#DEDEDE", "Page background, after Warp's grey"],
  ["Panel", "#E8E8E6", "Cards, code chips, quiet fills"],
  ["Ink", "#0E120F", "Text, dark sections, terminals"],
  ["Graft green", "#1F7A3A", "The one accent: links, actions, wins"],
  ["Deep root", "#0F3D1D", "Dark green fields, Integrate"],
  ["Sprout", "#B8F28C", "Highlights on dark, the version pill"],
  ["Dither leaf", "#6FA57E", "Dithered trees on the light ground"],
  ["Ember", "#FF975B", "Brows, branches, offset shadows. Sparingly"],
];

const ONE: TreeSpec[] = [{ x: 0.5, size: 0.95, seed: 7, depth: 9 }];
const PAIR: TreeSpec[] = [
  { x: 0.3, size: 0.85, lean: 0.08, seed: 21, depth: 9 },
  { x: 0.78, size: 0.6, lean: -0.1, seed: 3, depth: 8 },
];
const GROVE: TreeSpec[] = [
  { x: 0.12, size: 0.7, lean: 0.1, seed: 41, depth: 8 },
  { x: 0.5, size: 1, seed: 58, depth: 9 },
  { x: 0.88, size: 0.65, lean: -0.12, seed: 77, depth: 8 },
];

/** A poster after the reference sheet: stepped square tags, a numbered index and a dithered tree. */
function Poster({ variant }: { variant: "paper" | "field" | "sprout" }) {
  const ink = variant === "paper" ? "#1f7a3a" : variant === "field" ? "#b8f28c" : "#0f3d1d";
  return (
    <div className={`bp-poster bp-${variant}`}>
      <DitherTree className="bp-poster-dither" trees={variant === "field" ? PAIR : ONE} ink={ink} cell={3} light={variant === "field" ? 0.1 : 0} />
      <ol className="bp-index">
        <li>01 — Root</li>
        <li>02 — Graft</li>
        <li>03 — Delta</li>
        <li>04 — Integrate</li>
      </ol>
      <i className="bp-step s1" />
      <i className="bp-step s2" />
      <div className="bp-tags">
        <span>Radar</span>
        <span>Blueprints</span>
        <span>Bud &amp; Ring</span>
      </div>
      <div className="bp-plate">
        <b>graft</b>
        <span>{variant === "paper" ? "One root. Every agent its own graft." : variant === "field" ? "Costs only what the agent changes." : "Built on plain git."}</span>
        <em><Icon name="external" size={18} /></em>
      </div>
    </div>
  );
}

export const BRAND_SECTIONS: DocSection[] = [
  {
    title: "The mark",
    body: (
      <>
        <p>
          A green square with a face and a three-part tree growing out of its side: Root, and the
          grafts that grow from it. The eyes follow the pointer, blink, and change expression. Keep
          the tree breaking out of the square; never crop it to fit.
        </p>
        <div className="brand-marks">
          <div className="on-ground"><GraftLogo title="" /></div>
          <div className="on-ink"><GraftLogo title="" /></div>
          <div className="on-deep"><GraftLogo title="" expression="sleepy" /></div>
          <div className="on-panel"><GraftLogo title="" expression="surprised" /></div>
        </div>
        <ul className="brand-rules">
          <li><b>Do</b> place it straight on a surface: ground, ink or deep root.</li>
          <li><b>Do</b> keep clear space of half the square's width on every side.</li>
          <li><b>Don't</b> recolour the face, outline it, or put it on a busy dither.</li>
          <li><b>Don't</b> animate it faster: blinks are 130 ms, glances settle over 450 ms.</li>
        </ul>
      </>
    ),
  },
  {
    title: "Colour",
    body: (
      <>
        <p>
          A grey ground, ink for text, and one green. Sprout and ember are seasoning: a pill, a
          highlight, a shadow. If a screen has more than one strong colour moment, remove one.
        </p>
        <div className="brand-swatches">
          {SWATCHES.map(([name, hex, use]) => (
            <div key={hex} className="brand-swatch">
              <i style={{ background: hex }} />
              <b>{name}</b>
              <code>{hex}</code>
              <span>{use}</span>
            </div>
          ))}
        </div>
      </>
    ),
  },
  {
    title: "Dither",
    body: (
      <>
        <p>
          Graft's texture is an ordered (Bayer 8×8) dither of trees with branches, printed in one ink
          as square dots. Trees grow in when they enter the screen and sway slowly; a soft light
          crosses them so the dots shimmer. Use it as a field behind content, never behind body text
          without a quiet area cleared around it.
        </p>
        <div className="brand-dithers">
          <figure>
            <div className="bd-tile bd-ground"><DitherTree trees={GROVE} ink="#6fa57e" light={0.05} /></div>
            <figcaption>On ground · dither leaf ink · hero</figcaption>
          </figure>
          <figure>
            <div className="bd-tile bd-deep"><DitherTree trees={PAIR} ink="#b8f28c" cell={5} light={0.16} /></div>
            <figcaption>On deep root · sprout ink · terminal backdrop</figcaption>
          </figure>
          <figure>
            <div className="bd-tile bd-sprout"><DitherTree trees={ONE} ink="#0f3d1d" cell={3} light={0} /></div>
            <figcaption>On sprout · deep ink · fine pitch</figcaption>
          </figure>
        </div>
        <ul className="brand-rules">
          <li><b>Pitch</b> 3 px for small tiles, 4 px for sections, 5 px for dark fields.</li>
          <li><b>Ink</b> is always one colour from the palette; the ground shows through the gaps.</li>
          <li><b>Motion</b> pauses off screen and stops for reduced motion.</li>
        </ul>
      </>
    ),
  },
  {
    title: "Posters",
    body: (
      <>
        <p>
          For launches and social cards: a dithered tree, a numbered index in the corner, stepped
          square tags, and a solid plate carrying the wordmark and one line. Squares, never rounded
          corners.
        </p>
        <div className="brand-posters">
          <Poster variant="paper" />
          <Poster variant="field" />
          <Poster variant="sprout" />
        </div>
      </>
    ),
  },
  {
    title: "Type",
    body: (
      <div className="brand-type">
        <div>
          <span className="eyebrow">Display · Funnel Display 700–800</span>
          <p className="bt-display">One root. Every agent its own graft.</p>
        </div>
        <div>
          <span className="eyebrow">Text and labels · Geist Mono 400</span>
          <p className="bt-mono">Copy-on-write workspaces for parallel coding agents. Each graft is a view of one shared Root plus a private delta.</p>
        </div>
        <div>
          <span className="eyebrow">Eyebrows · Geist Mono, 0.12em, uppercase</span>
          <p className="bt-eyebrow">[01] What a worktree leaves to you</p>
        </div>
      </div>
    ),
  },
  {
    title: "Illustrations",
    body: (
      <>
        <p>
          Cut-paper shapes with a rough edge and grain: three leaf tiers, a branch, and the tree they
          make together. They peek in from the edges of a section, never sit in the middle of text.
        </p>
        <div className="brand-flora">
          {["tree", "branch", "leaf1", "leaf2", "leaf3", "branch-code"].map((k) => (
            <figure key={k}>
              <img src={`/${k}.svg`} alt="" />
              <figcaption>{k}.svg</figcaption>
            </figure>
          ))}
        </div>
      </>
    ),
  },
];
