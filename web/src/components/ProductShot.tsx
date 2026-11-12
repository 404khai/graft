// Illustrative session. Replace with captured `graft` output before launch.
const grafts = [
  { name: "auth", time: "49 ms", disk: "7.7 MiB" },
  { name: "api", time: "48 ms", disk: "7.6 MiB" },
  { name: "billing", time: "51 ms", disk: "7.7 MiB" },
];

export function ProductShot() {
  return (
    <section className="shot-wrap" aria-label="Example session">
      <div className="shot">
        <div className="term">
          <div className="term-dots"><i /><i /><i /></div>
          <div className="term-body">
            <p><span className="prompt">$</span> graft new auth &amp;&amp; graft new api &amp;&amp; graft new billing</p>
            {grafts.map((g) => (
              <p className="row" key={g.name}>
                <span className="ok">✓</span><b>{g.name}</b><span>base 0ff6509</span><span>{g.time}</span><span>{g.disk}</span>
              </p>
            ))}
            <p><span className="prompt">$</span> graft radar</p>
            <p className="row">
              <span className="warn">⚠ symbol</span><b>auth ↔ api</b><span>src/session.rs · fn refresh_token</span>
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}
