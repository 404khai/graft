import { BREW_INSTALL, CopyCommand } from "./CopyCommand";

export function Cta() {
  return (
    <section className="cta" id="install">
      <h2>Plant one Root.<br />Grow as many agents as you like.</h2>
      <CopyCommand command={`${BREW_INSTALL} && graft init`} />
    </section>
  );
}
