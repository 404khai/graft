import { InstallBox } from "./InstallBox";

export function Cta() {
  return (
    <section className="cta" id="install">
      <h2>Plant one Root.<br />Grow as many agents as you like.</h2>
      <InstallBox suffix=" && graft init" />
    </section>
  );
}
