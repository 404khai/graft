import { useState } from "react";
import { GraftLogo } from "./GraftLogo";

const links = [
  { label: "Docs", href: "/docs.html" },
  { label: "Features", href: "/#features" },
  { label: "Benchmarks", href: "/#at-work" },
  { label: "MCP", href: "/docs.html#mcp" },
  { label: "Changelog", href: "/changelog.html" },
];

export function Nav({ current }: { current?: "Docs" | "Changelog" }) {
  const [open, setOpen] = useState(false);

  return (
    <header className="nav">
      <a className="brand" href="/" aria-label="Graft home">
        <GraftLogo className="brand-logo" title="" />
        <span>graft</span>
      </a>
      <nav className={`nav-links${open ? " open" : ""}`} id="nav-links" aria-label="Primary">
        {links.map((l) => (
          <a
            key={l.label}
            href={l.href}
            aria-current={l.label === current ? "page" : undefined}
            onClick={() => setOpen(false)}
          >
            {l.label}
          </a>
        ))}
      </nav>
      <div className="nav-actions">
        <a className="muted-link" href="https://github.com/404khai/graft">GitHub</a>
        <a className="btn btn-dark btn-sm" href="/#install">brew install graft</a>
      </div>
      <button
        className="nav-toggle"
        aria-expanded={open}
        aria-controls="nav-links"
        aria-label="Menu"
        onClick={() => setOpen((o) => !o)}
      >
        <span />
        <span />
      </button>
    </header>
  );
}
