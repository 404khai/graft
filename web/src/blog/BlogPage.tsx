import { Icon } from "../components/Icon";
import { useEffect, useState, type ReactNode } from "react";
import { Footer } from "../components/Footer";
import { Nav } from "../components/Nav";
import { DitherTree, type TreeSpec } from "../components/art/DitherTree";
import { copyText } from "../components/CopyCommand";
import { ScrollIndicator } from "../components/nexvyn/ScrollIndicator";
import { figures, type FigureId } from "../generated/figures";
import { BarChart } from "./BarChart";
import { AUTHOR, POSTS, postBySlug, readingMinutes, toMarkdown, type Block, type Post } from "./posts";
import { SITE } from "../site";
import "../generated/anatomy.css";
import "./blog.css";

const fmt = (iso: string) =>
  new Date(`${iso}T12:00:00Z`).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });

/** `code`, **bold** and [text](href) inside a paragraph. */
function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\(([^)]+)\)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1]) out.push(<code key={m.index}>{m[1]}</code>);
    else if (m[2]) out.push(<b key={m.index}>{m[2]}</b>);
    else out.push(<a key={m.index} href={m[4]}>{m[3]}</a>);
    last = re.lastIndex;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function Figure({ id, className }: { id: FigureId; className?: string }) {
  return <div className={`iso blog-figure${className ? ` ${className}` : ""}`} data-theme="light" dangerouslySetInnerHTML={{ __html: figures[id] }} />;
}

/** A Brimble-style cover: the figure on grid paper with an italic title in the corner. */
function Cover({ post, large }: { post: Post; large?: boolean }) {
  return (
    <div className={`blog-cover${large ? " large" : ""}`}>
      <div className="blog-cover-rule" aria-hidden="true" />
      <Figure id={post.figure} />
      <div className="blog-cover-title">
        <span>— {post.category}</span>
        <b>{post.cover}</b>
      </div>
    </div>
  );
}

function Meta({ post }: { post: Post }) {
  return (
    <div className="blog-meta">
      <span>{AUTHOR.name}</span>
      <i />
      <time dateTime={post.date}>{fmt(post.date)}</time>
      <i />
      <span>{readingMinutes(post)} min read</span>
    </div>
  );
}

const HEADER_TREES: TreeSpec[] = [
  { x: 0.86, size: 1.1, lean: -0.08, seed: 91, depth: 9 },
  { x: 0.99, size: 0.6, lean: -0.2, seed: 12, depth: 8 },
];

function BlogIndex() {
  const [featured, ...rest] = [...POSTS].sort((a, b) => b.date.localeCompare(a.date));
  return (
    <main className="blog">
      <header className="blog-head">
        <DitherTree className="blog-head-dither" trees={HEADER_TREES} ink="#6fa57e" light={0} cell={4} />
        <h1>Graft Blog</h1>
        <p>Engineering notes on the decisions behind Graft: filesystems, journals, git internals and the numbers that settled them.</p>
      </header>

      <a className="blog-featured" href={`/blog/${featured.slug}`}>
        <Cover post={featured} large />
        <div className="blog-featured-copy">
          <span className="blog-cat">{featured.category}</span>
          <h2>{featured.title}</h2>
          <p>{featured.summary}</p>
          <Meta post={featured} />
        </div>
      </a>

      <div className="blog-grid">
        {rest.map((p) => (
          <a key={p.slug} className="blog-card" href={`/blog/${p.slug}`}>
            <Cover post={p} />
            <span className="blog-cat">{p.category}</span>
            <h3>{p.title}</h3>
            <p>{p.summary}</p>
            <Meta post={p} />
          </a>
        ))}
      </div>
    </main>
  );
}

function BlockView({ block, index }: { block: Block; index: number }) {
  if ("p" in block) return <p>{inline(block.p)}</p>;
  if ("h2" in block)
    return (
      <h2 id={block.h2.toLowerCase().replace(/\W+/g, "-")} data-section-index={index}>
        {block.h2}
      </h2>
    );
  if ("code" in block)
    return (
      <pre className="blog-code" data-lang={block.lang}>
        <code>{block.code}</code>
      </pre>
    );
  if ("figure" in block)
    return (
      <figure className="blog-fig">
        <Figure id={block.figure} />
        <figcaption>{block.caption}</figcaption>
      </figure>
    );
  if ("list" in block)
    return (
      <ul>
        {block.list.map((l) => (
          <li key={l}>{inline(l)}</li>
        ))}
      </ul>
    );
  if ("table" in block)
    return (
      <figure className="blog-table">
        <div className="blog-table-scroll">
          <table>
            <thead>
              <tr>{block.table.head.map((h, i) => <th key={i} scope="col">{h}</th>)}</tr>
            </thead>
            <tbody>
              {block.table.rows.map((r, i) => (
                <tr key={i}>{r.map((c, j) => (j === 0 ? <th key={j} scope="row">{inline(c)}</th> : <td key={j}>{inline(c)}</td>))}</tr>
              ))}
            </tbody>
          </table>
        </div>
        {block.table.caption && <figcaption>{block.table.caption}</figcaption>}
      </figure>
    );
  if ("chart" in block) return <BarChart chart={block.chart} />;
  return (
    <div className="blog-stats">
      {block.stats.map((s) => (
        <div key={s.label}>
          <b>{s.value}</b>
          <span>{s.label}</span>
          {s.note && <small>{s.note}</small>}
        </div>
      ))}
    </div>
  );
}

function Ask({ post }: { post: Post }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(t);
  }, [copied]);
  const url = `${SITE}/blog/${post.slug}`;
  const q = encodeURIComponent(`Read ${url} and help me understand it.`);
  return (
    <div className="blog-ask">
      <span>Ask about this post</span>
      <a href={`https://claude.ai/new?q=${q}`} target="_blank" rel="noreferrer">
        <img src="/claude.svg" alt="" />Claude
      </a>
      <a href={`https://chatgpt.com/?q=${q}`} target="_blank" rel="noreferrer">
        <img src="/chatgpt.svg" alt="" />ChatGPT
      </a>
      <button type="button" onClick={async () => setCopied(await copyText(toMarkdown(post, url)))}>
        {copied ? "Copied" : "Copy as Markdown"}
      </button>
    </div>
  );
}

function PostView({ post }: { post: Post }) {
  useEffect(() => {
    document.title = `${post.title} · Graft blog`;
  }, [post]);
  // Section 0 is the top of the article; each h2 is the next section.
  let n = 0;
  const sections = [{ id: "intro", title: "Intro" }, ...post.body.flatMap((b) => ("h2" in b ? [{ id: b.h2, title: b.h2 }] : []))];
  const others = POSTS.filter((p) => p.slug !== post.slug);
  return (
    <main className="blog blog-post">
      <article>
        <a className="blog-back" href="/blog"><Icon name="arrow-left" size={14} /> All posts</a>
        <div className="blog-post-head" data-section-index={0}>
          <div className="blog-kicker">
            <span className="blog-cat">{post.category}</span>
            <span>· {fmt(post.date).toUpperCase()} · {readingMinutes(post)} MIN READ</span>
          </div>
          <h1>{post.title}</h1>
          <p className="blog-lede">{post.summary}</p>
          <a className="blog-author" href={AUTHOR.href}>
            <img src={AUTHOR.avatar} alt="" width="40" height="40" />
            <span>
              <b>{AUTHOR.name}</b>
              <small>{AUTHOR.handle}</small>
            </span>
          </a>
          <Ask post={post} />
        </div>
        <Cover post={post} large />
        <div className="blog-body">
          {post.body.map((b, i) => (
            <BlockView key={i} block={b} index={"h2" in b ? ++n : -1} />
          ))}
        </div>
        <aside className="blog-sources">
          <b>Sources</b>
          {post.sources.map((s) => (
            <a key={s.href} href={s.href}>{s.label} <Icon name="external" size={14} /></a>
          ))}
        </aside>
        <div className="blog-more">
          <span className="blog-cat">Keep reading</span>
          {others.map((p) => (
            <a key={p.slug} href={`/blog/${p.slug}`}>
              <b>{p.title}</b>
              <span>{fmt(p.date)}</span>
            </a>
          ))}
        </div>
      </article>
      <aside className="blog-rail" aria-label="On this page">
        <ScrollIndicator sections={sections} />
      </aside>
    </main>
  );
}

export function Blog({ slug }: { slug?: string }) {
  const post = slug ? postBySlug(slug) : undefined;
  return (
    <div className="rails">
      <Nav current="Blog" />
      {post ? <PostView post={post} /> : <BlogIndex />}
      <Footer />
    </div>
  );
}
