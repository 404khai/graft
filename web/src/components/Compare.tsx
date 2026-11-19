import { Icon } from "./Icon";
/**
 * Grafts vs worktrees, from the README's measured results: bench run
 * 42113580319, GitHub Actions ubuntu-latest (AMD EPYC 9V74, 4 threads), XFS
 * with reflinks, three runs, medians, 16 agents. Every value is a measured
 * number; the best in each column per fixture is highlighted, wins or not.
 */
type Row = { method: "worktree" | "graft (reflink)" | "graft (FUSE)"; create: string; ready: string; disk: string; prune: string };
type Fixture = { name: string; files: string; rows: Row[] };

const FIXTURES: Fixture[] = [
  {
    name: "small",
    files: "1k files",
    rows: [
      { method: "worktree", create: "75 ms", ready: "425 ms", disk: "124.4 MiB", prune: "32 ms" },
      { method: "graft (reflink)", create: "123 ms", ready: "250 ms", disk: "11.8 MiB", prune: "46 ms" },
      { method: "graft (FUSE)", create: "19 ms", ready: "141 ms", disk: "1.2 MiB", prune: "6 ms" },
    ],
  },
  {
    name: "medium",
    files: "20k files",
    rows: [
      { method: "worktree", create: "1.29 s", ready: "2.57 s", disk: "3.0 GiB", prune: "1.12 s" },
      { method: "graft (reflink)", create: "1.78 s", ready: "1.91 s", disk: "326.1 MiB", prune: "1.15 s" },
      { method: "graft (FUSE)", create: "19 ms", ready: "153 ms", disk: "45.0 MiB", prune: "6 ms" },
    ],
  },
  {
    name: "large",
    files: "200k files",
    rows: [
      { method: "worktree", create: "15.12 s", ready: "15.56 s", disk: "22.7 GiB", prune: "7.63 s" },
      { method: "graft (reflink)", create: "10.55 s", ready: "10.65 s", disk: "1.9 GiB", prune: "6.97 s" },
      { method: "graft (FUSE)", create: "18 ms", ready: "135 ms", disk: "1.1 MiB", prune: "6 ms" },
    ],
  },
];

const COLS = [
  { key: "create", label: "Create" },
  { key: "ready", label: "Ready, later agents" },
  { key: "disk", label: "Disk added (16)" },
  { key: "prune", label: "Prune" },
] as const;

const UNITS: Record<string, number> = { ms: 1, s: 1000, MiB: 1, GiB: 1024 };
const num = (v: string) => {
  const [n, u] = v.split(" ");
  return Number(n) * (UNITS[u] ?? 1);
};

export function Compare() {
  return (
    <section className="compare" id="compare">
      <div className="section-head">
        <div>
          <span className="eyebrow">[02] Grafts vs worktrees</span>
          <h2>Measured, not promised.</h2>
        </div>
        <p>
          16 agents per fixture, each creating a workspace and running a passing <code>npm test</code>.
          Medians of three runs on XFS with reflinks.
        </p>
      </div>

      <div className="compare-scroll">
        <table className="compare-table">
          <thead>
            <tr>
              <th scope="col">Fixture</th>
              <th scope="col">Method</th>
              {COLS.map((c) => <th key={c.key} scope="col">{c.label}</th>)}
            </tr>
          </thead>
          {FIXTURES.map((f) => {
            const best = Object.fromEntries(
              COLS.map((c) => [c.key, Math.min(...f.rows.map((r) => num(r[c.key])))]),
            );
            return (
              <tbody key={f.name}>
                {f.rows.map((r, i) => (
                  <tr key={r.method} className={r.method === "worktree" ? "is-worktree" : undefined}>
                    {i === 0 && (
                      <th scope="rowgroup" rowSpan={f.rows.length} className="compare-fixture">
                        <b>{f.name}</b>
                        <span>{f.files}</span>
                      </th>
                    )}
                    <td className="compare-method">{r.method}</td>
                    {COLS.map((c) => (
                      <td key={c.key} className={num(r[c.key]) === best[c.key] ? "is-best" : undefined}>
                        {r[c.key]}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            );
          })}
        </table>
      </div>

      <div className="compare-notes">
        <p>
          <b>Where grafts lose:</b> reflink grafts create slower than worktrees on small and medium repos,
          and the first graft is slower because it builds the shared <code>node_modules</code> layer.
          Inside a workspace, FUSE grafts are up to about 40× slower on file-heavy work like <code>rg</code>.
        </p>
        <a href="https://github.com/404khai/graft/tree/main/bench/results">
          Bench run 42113580319 · GitHub Actions ubuntu-latest, AMD EPYC 9V74 · full tables <Icon name="arrow-right" size={14} />
        </a>
      </div>
    </section>
  );
}
