import { Icon } from "./Icon";
import { DitherTree, type TreeSpec } from "./art/DitherTree";
// Illustrative session. Replace with captured `graft` output before launch.
const grafts = [
  { name: "auth", time: "49 ms", disk: "7.7 MiB" },
  { name: "api", time: "48 ms", disk: "7.6 MiB" },
  { name: "billing", time: "51 ms", disk: "7.7 MiB" },
];

const SHOT_TREES: TreeSpec[] = [
  { x: 0.03, size: 0.8, lean: 0.16, seed: 41, depth: 9 },
  { x: 0.97, size: 0.85, lean: -0.14, seed: 58, depth: 9 },
];

export function ProductShot() {
  return (
    <section className="shot-wrap" aria-label="Example session">
      <div className="shot">
        <DitherTree className="shot-dither" trees={SHOT_TREES} ink="#b8f28c" cell={5} light={0.16} />
        <div className="term">
          <div className="term-dots"><i /><i /><i /></div>
          <div className="term-body">
            <p><span className="prompt">$</span> graft new auth &amp;&amp; graft new api &amp;&amp; graft new billing</p>
            {grafts.map((g) => (
              <p className="row" key={g.name}>
                <span className="ok"><Icon name="check" size={15} /></span><b>{g.name}</b><span>base 0ff6509</span><span>{g.time}</span><span>{g.disk}</span>
              </p>
            ))}
            <p><span className="prompt">$</span> graft radar</p>
            <p className="row">
              <span className="warn"><Icon name="alert" size={15} /> symbol</span><b className="pair">auth <Icon name="both-ways" size={14} /> api</b><span>src/session.rs · fn refresh_token</span>
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}
