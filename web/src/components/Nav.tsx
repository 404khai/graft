import { Icon } from "./Icon";
import { useEffect, useState } from "react";
import { BREW_INSTALL, CopyCommand } from "./CopyCommand";
import { GraftLogo } from "./GraftLogo";

const links = [
  { label: "Docs", href: "/docs" },
  { label: "Features", href: "/#features" },
  { label: "Benchmarks", href: "/#at-work" },
  { label: "MCP", href: "/docs/mcp" },
  { label: "Blog", href: "/blog" },
  { label: "Changelog", href: "/changelog" },
];

export function Nav({ current }: { current?: "Docs" | "Changelog" | "Blog" }) {
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
        <GitHubLink />
        <CopyCommand command={BREW_INSTALL} className="btn btn-dark btn-sm" prompt={false} />
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

const REPO = "404khai/graft";
const STARS_KEY = "graft:stars";

/** GitHub mark with the repo's star count, fetched once per session. */
function GitHubLink() {
  // Read after mount, so the prerendered HTML and the first client render match.
  const [stars, setStars] = useState<number | null>(null);
  useEffect(() => {
    try {
      const v = sessionStorage.getItem(STARS_KEY);
      if (v) {
        setStars(Number(v));
        return;
      }
    } catch {
      // storage blocked: fetch instead
    }
    const ctl = new AbortController();
    fetch(`https://api.github.com/repos/${REPO}`, { signal: ctl.signal })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        if (typeof d?.stargazers_count !== "number") return;
        setStars(d.stargazers_count);
        try {
          sessionStorage.setItem(STARS_KEY, String(d.stargazers_count));
        } catch {
          // storage blocked: the count just isn't cached
        }
      })
      .catch(() => {});
    return () => ctl.abort();
  }, []);
  const fmt = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n));
  return (
    <a
      className="gh-link"
      href={`https://github.com/${REPO}`}
      aria-label={stars === null ? "Graft on GitHub" : `Graft on GitHub, ${stars} stars`}
    >
      <img src="/github.svg" alt="" width="18" height="18" />
      {stars !== null && (
        <span className="gh-stars">
          <Icon name="star" size={13} />
          {fmt(stars)}
        </span>
      )}
    </a>
  );
}
