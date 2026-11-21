// Renders every route to static HTML after `vite build`, so the site works
// with JavaScript off and crawlers, link previews and AI agents see real
// content. Also writes llms.txt, llms-full.txt, a Markdown copy of each blog
// post, sitemap.xml and robots.txt. Run by `npm run build`.
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = join(HERE, "../dist");
const SSR = join(HERE, "../dist-ssr");
const { render, ROUTES, POSTS, SITE, toMarkdown } = await import(pathToFileURL(join(SSR, "entry-server.js")).href);

const template = readFileSync(join(DIST, "index.html"), "utf8");
const esc = (s) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

// Preload the Latin subsets of both fonts so text paints in its final face.
const fonts = readdirSync(join(DIST, "assets")).filter((f) => /(funnel-display|geist-mono)-latin-wght-normal.*\.woff2$/.test(f));
const preload = fonts.map((f) => `<link rel="preload" href="/assets/${f}" as="font" type="font/woff2" crossorigin />`).join("\n    ");

function head(route) {
  const url = `${SITE}${route.path === "/" ? "/" : route.path}`;
  return [
    `<title>${esc(route.title)}</title>`,
    `<meta name="description" content="${esc(route.description)}" />`,
    `<link rel="canonical" href="${url}" />`,
    `<meta property="og:type" content="${route.type ?? "website"}" />`,
    `<meta property="og:site_name" content="Graft" />`,
    `<meta property="og:title" content="${esc(route.title)}" />`,
    `<meta property="og:description" content="${esc(route.description)}" />`,
    `<meta property="og:url" content="${url}" />`,
    `<meta property="og:image" content="${SITE}/og.png" />`,
    `<meta property="og:image:width" content="1200" />`,
    `<meta property="og:image:height" content="630" />`,
    `<meta property="og:image:alt" content="Graft: one root, every agent its own graft." />`,
    `<meta name="twitter:card" content="summary_large_image" />`,
    `<meta name="twitter:title" content="${esc(route.title)}" />`,
    `<meta name="twitter:description" content="${esc(route.description)}" />`,
    `<meta name="twitter:image" content="${SITE}/og.png" />`,
    `<link rel="alternate" type="text/plain" title="llms.txt" href="/llms.txt" />`,
    preload,
  ].join("\n    ");
}

const text = (html) =>
  html
    .replace(/<(script|style|svg|canvas)[\s\S]*?<\/\1>/g, " ")
    .replace(/<\/(p|h[1-6]|li|tr|div|section|pre|figcaption|dt|dd)>/g, "\n")
    .replace(/<br\s*\/?>/g, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ").replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#x27;|&#39;/g, "'")
    .replace(/[ \t]+/g, " ")
    .replace(/\n\s*\n\s*/g, "\n\n")
    .trim();

const bodies = new Map();
for (const route of ROUTES) {
  const app = render(route);
  bodies.set(route.path, app);
  const html = template.replace(/<title>[\s\S]*?<\/title>\n?\s*/, "").replace("<!--head-->", head(route)).replace("<!--app-->", app);
  // /docs/radar is written as docs/radar.html (served at /docs/radar with Vercel's cleanUrls)
  // and docs/radar/index.html (for hosts that resolve directory indexes).
  const outs = route.path === "/" ? [join(DIST, "index.html")] : [join(DIST, `${route.path.slice(1)}.html`), join(DIST, route.path.slice(1), "index.html")];
  for (const out of outs) {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, html);
  }
}

// Markdown copies of posts, for agents.
for (const post of POSTS) {
  writeFileSync(join(DIST, "blog", `${post.slug}.md`), toMarkdown(post, `${SITE}/blog/${post.slug}`));
}

// llms.txt (https://llmstxt.org): what Graft is and where everything lives.
const byPrefix = (p) => ROUTES.filter((r) => r.path.startsWith(p) && r.path !== p.replace(/\/$/, ""));
const line = (r, href = `${SITE}${r.path}`) => `- [${r.title.replace(/ · Graft (docs|blog)$/, "")}](${href}): ${r.description}`;
const llms = [
  "# Graft",
  "",
  `> ${ROUTES[0].description}`,
  "",
  "Graft is a CLI, a daemon (graftd) and an MCP server. Install: `brew install 404khai/graft/graft`. Source: https://github.com/404khai/graft",
  "",
  "## Docs",
  ...byPrefix("/docs/").map((r) => line(r)),
  "",
  "## Blog",
  ...POSTS.map((p) => `- [${p.title}](${SITE}/blog/${p.slug}.md): ${p.summary}`),
  "",
  "## Optional",
  `- [Everything as one text file](${SITE}/llms-full.txt)`,
  `- [Changelog](${SITE}/changelog): every release, linked to its pull request`,
  "",
].join("\n");
writeFileSync(join(DIST, "llms.txt"), llms);

const main = (html) => html.match(/<main[^>]*>([\s\S]*)<\/main>/)?.[1] ?? html;
const full = [
  `# Graft: full text of the website\n\n${ROUTES[0].description}`,
  ...byPrefix("/docs/").map((r) => `## ${r.title}\n\nSource: ${SITE}${r.path}\n\n${text(main(bodies.get(r.path)))}`),
  ...POSTS.map((p) => `## Blog: ${p.title}\n\n${toMarkdown(p, `${SITE}/blog/${p.slug}`)}`),
  `## Changelog\n\nSource: ${SITE}/changelog\n\n${text(main(bodies.get("/changelog")))}`,
].join("\n\n---\n\n");
writeFileSync(join(DIST, "llms-full.txt"), full);

writeFileSync(
  join(DIST, "sitemap.xml"),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${ROUTES.map((r) => `  <url><loc>${SITE}${r.path}</loc></url>`).join("\n")}\n</urlset>\n`,
);
writeFileSync(join(DIST, "robots.txt"), `User-agent: *\nAllow: /\n\nSitemap: ${SITE}/sitemap.xml\n`);

rmSync(SSR, { recursive: true, force: true });
console.log(`prerendered ${ROUTES.length} routes, ${POSTS.length} post markdown files, llms.txt, llms-full.txt, sitemap.xml, robots.txt; preloaded ${fonts.length} fonts`);
