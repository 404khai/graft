# Graft website

Vite + React + TypeScript. Design source: the "Graft — Home v2" artboard in Paper.

```sh
npm install
npm run dev      # http://localhost:5173
npm run build    # type-check and build to dist/
npm run figures  # regenerate the blog's Anatomy figures (src/generated/)
```

Three pages: `index.html` (home), `docs.html` (`src/docs/`, hash-routed: `#quickstart`, `#commands`…) and `changelog.html` (`src/changelog/`).

- `src/docs/content.tsx`: every docs page and section; add a page to `PAGES` and it appears in the sidebar, search and overview grid.
- `src/changelog/main.tsx`: `ENTRIES`, newest first, one per merged PR or group of PRs.
- `src/components/nexvyn/`: NavigationCompass, BounceSidebar and ScrollIndicator from Nexvyn UI, as adapted (no `motion` dependency) in Astrid.
- `src/components/Flora.tsx`: the `public/` illustrations, placed so they peek in from an edge.
- `src/styles.css`: design tokens (`:root`) and all styles; breakpoints at 1100, 900 and 640px.
- `src/components/`: one component per page section.
- Benchmark figures in `AtWork.tsx` come from `bench/results` (run 42113580319). The terminal in `ProductShot.tsx` is illustrative.

## Routes

One app (`index.html`, `src/main.tsx`): `/`, `/docs` (hash routes such as `/docs#quickstart`), `/changelog`, `/blog` and `/blog/<slug>`. The host must rewrite unknown paths to `/index.html`: `vercel.json` does this on Vercel (static files are still served first), and `npm run dev` and `npm run preview` already do.

- `src/blog/posts.ts`: blog posts as blocks (render as HTML, copy as Markdown). Figures are built by `scripts/build-figures.mjs` with the vendored Anatomy kit (`vendor/anatomy/`, MIT).
- `src/docs/brand.tsx`: the brand guidelines page.
- `src/components/art/DitherTree.tsx`: the Bayer-dithered, growing trees.
- `src/components/Icon.tsx`: Hugeicons used across the site.
