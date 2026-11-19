import { GraftLogo } from "./GraftLogo";
import { Poster } from "./Poster";

const gh = "https://github.com/404khai/graft";
const columns = [
  { title: "Product", links: [["Blueprints", "/docs#blueprints"], ["Radar", "/docs#radar"], ["Integrate", "/docs#integrate"], ["MCP server", "/docs#mcp"]] },
  { title: "Resources", links: [["Docs", "/docs"], ["Benchmarks", `${gh}/tree/main/bench/results`], ["On‑disk format", `${gh}/blob/main/docs/on-disk-format.md`], ["Changelog", "/changelog"]] },
  { title: "Community", links: [["GitHub", gh], ["Issues", `${gh}/issues`], ["Releases", `${gh}/releases`]] },
];

export function Footer() {
  return (
    <footer className="footer">
      <Poster layout="footer">
      <div className="footer-links">
        <p className="footer-tag">Copy‑on‑write workspaces<br />for parallel agents.</p>
        {columns.map((c) => (
          <div key={c.title}>
            <span>{c.title}</span>
            {c.links.map(([label, href]) => <a key={label} href={href}>{label}</a>)}
          </div>
        ))}
      </div>
      <div className="wordmark">
        <GraftLogo className="wordmark-logo" />
        <span aria-hidden="true">graft</span>
      </div>
      <div className="footer-bar">
        <div className="footer-meta"><span>© 2026 built by <a className="footer-author" href="https://404khai.vercel.app">404khai</a></span><span>Linux · macOS</span></div>
        <div className="badges">
          <span className="badge-box"><i className="blocks" aria-hidden="true"><b /><b /><b /></i> 0.1.1 live now</span>
          <span className="badge-git"><img src="/git.png" alt="" width={20} height={20} /> Built on plain git</span>
        </div>
      </div>
      </Poster>
    </footer>
  );
}
