import { StrictMode, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { createRoot } from "react-dom/client";
import { Flora } from "../components/Flora";
import { GraftLogo } from "../components/GraftLogo";
import { BounceSidebar } from "../components/nexvyn/BounceSidebar";
import { NavigationCompass } from "../components/nexvyn/NavigationCompass";
import { ScrollIndicator } from "../components/nexvyn/ScrollIndicator";
import { BlueprintSheet } from "../components/art/BlueprintSheet";
import { RadarScope } from "../components/art/RadarScope";
import { GH, PAGES, type DocPage } from "./content";
import "../styles.css";
import "./docs.css";

const ROUTES = [{ id: "overview", title: "Overview" }, ...PAGES.map((p) => ({ id: p.id, title: p.title }))];
const TABS = [
  { label: "Documentation", id: "overview" },
  { label: "Commands", id: "commands" },
  { label: "MCP", id: "mcp" },
];

const fromHash = () => {
  const id = location.hash.slice(1);
  return ROUTES.some((r) => r.id === id) ? id : "overview";
};

/** The illustration on a page's tile and header. */
function TileArt({ page }: { page: DocPage }) {
  if (page.id === "radar")
    return (
      <RadarScope className="doc-tile-radar">
        <img className="radar-leaf" src="/leaf2.svg" alt="" />
      </RadarScope>
    );
  if (page.id === "blueprints") return <BlueprintSheet className="doc-tile-blueprint" />;
  if (page.art === "graft") return <GraftLogo className="doc-tile-art" title="" />;
  return <img className="doc-tile-art" src={`/${page.art}.svg`} alt="" />;
}

function Overview() {
  return (
    <>
      <section className="doc-banner" data-section-index={0}>
        <div className="doc-banner-flora" aria-hidden="true">
          <Flora kind="tree" className="dbf-tree" />
          <Flora kind="branch" className="dbf-branch" style={{ "--r": "-18deg", "--d": "-3s" }} />
          <Flora kind="leaf1" className="dbf-leaf1" style={{ "--r": "-150deg", "--d": "-1s" }} />
          <Flora kind="leaf3" className="dbf-leaf3" style={{ "--r": "20deg", "--d": "-5s" }} />
        </div>
        <div className="doc-banner-copy">
          <span className="eyebrow">Graft / Docs</span>
          <h1>Graft documentation</h1>
          <p>
            Graft gives every coding agent a copy‑on‑write workspace on one shared Root, a private
            runtime, live overlap detection and a tested path back into git. Drag the dial, or just
            scroll.
          </p>
        </div>
        <NavigationCompass
          className="doc-compass"
          links={[
            { angle: 345, label: "Quickstart", href: "#quickstart" },
            { angle: 75, label: "Commands", href: "#commands" },
            { angle: 165, label: "MCP", href: "#mcp" },
            { angle: 255, label: "Concepts", href: "#concepts" },
          ]}
        />
      </section>

      <section className="doc-section" data-section-index={1}>
        <span className="eyebrow">Start here</span>
        <div className="doc-grid">
          {PAGES.map((p) => (
            <a key={p.id} className="doc-tile" href={`#${p.id}`}>
              <div className={`doc-tile-stage art-${p.art}`}>
                <TileArt page={p} />
              </div>
              <div className="doc-tile-copy">
                <b>{p.title}</b>
                <span>{p.summary}</span>
              </div>
            </a>
          ))}
        </div>
      </section>
    </>
  );
}

function Page({ page }: { page: DocPage }) {
  const next = PAGES[PAGES.indexOf(page) + 1];
  return (
    <>
      <header className="doc-page-head">
        {page.id === "radar" || page.id === "blueprints" ? (
          <div className="doc-page-art"><TileArt page={page} /></div>
        ) : (
          <Flora kind={page.art === "graft" ? "leaf2" : page.art} className="doc-page-peek" style={{ "--r": "-14deg" }} />
        )}
        <span className="eyebrow">{page.group}</span>
        <h1>{page.title}</h1>
        <p>{page.summary}</p>
      </header>
      {page.sections.map((s, i) => (
        <section key={s.title} className="doc-section doc-prose" data-section-index={i} id={s.title.toLowerCase().replace(/\W+/g, "-")}>
          <h2>{s.title}</h2>
          {s.body}
        </section>
      ))}
      {next && (
        <a className="doc-next" href={`#${next.id}`}>
          <span>Next</span>
          <b>{next.title} →</b>
        </a>
      )}
    </>
  );
}

function Search({ onGo }: { onGo: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!open) return;
    input.current?.focus();
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = prev;
      trigger.current?.focus();
    };
  }, [open]);

  const hits = useMemo(() => {
    const term = q.trim().toLowerCase();
    if (!term) return PAGES;
    return PAGES.filter((p) =>
      [p.title, p.summary, ...p.sections.map((s) => s.title)].join(" ").toLowerCase().includes(term),
    );
  }, [q]);

  const close = () => {
    setOpen(false);
    setQ("");
    setSel(0);
  };
  const go = (id: string) => {
    onGo(id);
    close();
  };

  return (
    <>
      <button ref={trigger} className="doc-search" onClick={() => setOpen(true)} aria-haspopup="dialog">
        <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="9" cy="9" r="6" /><path d="m14 14 4 4" /></svg>
        <span>Search…</span>
        <kbd>⌘K</kbd>
      </button>

      {open && createPortal(
        <div className="search-overlay" onMouseDown={(e) => e.target === e.currentTarget && close()}>
          <div className="search-dialog" role="dialog" aria-modal="true" aria-label="Search the docs">
            <Flora kind="tree" className="sd-tree-l" />
            <Flora kind="tree" className="sd-tree-r" style={{ "--d": "-3s" }} />
            <Flora kind="leaf2" className="sd-leaf" style={{ "--r": "150deg", "--d": "-1s" }} />
            <div className="search-box">
              <div className="search-field">
                <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="9" cy="9" r="6" /><path d="m14 14 4 4" /></svg>
                <input
                  ref={input}
                  value={q}
                  onChange={(e) => {
                    setQ(e.target.value);
                    setSel(0);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") close();
                    else if (e.key === "ArrowDown") {
                      e.preventDefault();
                      setSel((i) => Math.min(i + 1, hits.length - 1));
                    } else if (e.key === "ArrowUp") {
                      e.preventDefault();
                      setSel((i) => Math.max(i - 1, 0));
                    } else if (e.key === "Enter" && hits[sel]) go(hits[sel].id);
                  }}
                  placeholder="Search the docs…"
                  aria-label="Search the docs"
                  aria-controls="search-results"
                  aria-activedescendant={hits[sel] ? `hit-${hits[sel].id}` : undefined}
                />
                <kbd>esc</kbd>
              </div>
              <ul className="search-results" id="search-results" role="listbox">
                {hits.length ? (
                  hits.map((h, i) => (
                    <li key={h.id} id={`hit-${h.id}`} role="option" aria-selected={i === sel}>
                      <button onMouseEnter={() => setSel(i)} onClick={() => go(h.id)}>
                        <span className="hit-group">{h.group}</span>
                        <b>{h.title}</b>
                        <span className="hit-summary">{h.summary}</span>
                      </button>
                    </li>
                  ))
                ) : (
                  <li className="empty">Nothing matches “{q}”.</li>
                )}
              </ul>
              <div className="search-foot"><span>↑↓ to move</span><span>↵ to open</span><span>esc to close</span></div>
            </div>
          </div>
        </div>,
        document.body,
      )}
    </>
  );
}

function Docs() {
  const [route, setRoute] = useState(fromHash);
  const [menu, setMenu] = useState(false);
  useEffect(() => {
    const onHash = () => {
      setRoute(fromHash());
      setMenu(false);
      window.scrollTo(0, 0);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const go = (id: string) => {
    location.hash = id;
  };
  const page = PAGES.find((p) => p.id === route);
  const sections = page ? page.sections.map((s) => s.title) : ["Overview", "Start here"];
  const index = ROUTES.findIndex((r) => r.id === route);

  return (
    <div className="docs">
      <aside className={`docs-side${menu ? " open" : ""}`}>
        <div className="docs-side-top">
          <a className="docs-brand" href="/">
            <GraftLogo className="docs-brand-logo" title="" />
            <span>graft</span>
            <small>Docs</small>
          </a>
          <button className="docs-menu" aria-expanded={menu} aria-label="Docs menu" onClick={() => setMenu((m) => !m)}>
            <span /><span />
          </button>
        </div>
        <div className="docs-side-body">
          <Search onGo={go} />
          <BounceSidebar items={ROUTES.map((r) => r.title)} value={index} onChange={(i) => go(ROUTES[i].id)} />
          <nav className="docs-out">
            <a href="/changelog.html">Changelog</a>
            <a href={GH}>GitHub ↗</a>
            <a href="/">Home ↗</a>
          </nav>
        </div>
      </aside>

      <div className="docs-body">
        <nav className="docs-tabs" aria-label="Docs sections">
          {TABS.map((t) => (
            <a key={t.id} href={`#${t.id}`} aria-current={route === t.id ? "page" : undefined}>{t.label}</a>
          ))}
        </nav>
        <main className="docs-main" key={route}>
          {page ? <Page page={page} /> : <Overview />}
        </main>
      </div>

      <aside className="docs-rail" aria-label="On this page">
        <ScrollIndicator key={route} sections={sections.map((title) => ({ id: title, title }))} />
      </aside>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Docs />
  </StrictMode>,
);
