import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

// One app, three routes. Docs and changelog load on demand.
const Docs = lazy(() => import("./docs/DocsPage").then((m) => ({ default: m.Docs })));
const Blog = lazy(() => import("./blog/BlogPage").then((m) => ({ default: m.Blog })));
const Changelog = lazy(() => import("./changelog/ChangelogPage").then((m) => ({ default: m.Changelog })));

const ROUTES: Record<string, { title: string; page: React.ReactNode }> = {
  "/": { title: "Graft — copy-on-write workspaces for agents", page: <App /> },
  "/docs": { title: "Graft docs", page: <Docs /> },
  "/changelog": { title: "Graft changelog", page: <Changelog /> },
  "/blog": { title: "Graft blog", page: <Blog /> },
};

const path = location.pathname.replace(/\/+$/, "") || "/";
const post = path.match(/^\/blog\/([a-z0-9-]+)$/)?.[1];
const route = post
  ? { title: "Graft blog", page: <Blog slug={post} /> }
  : (ROUTES[path] ?? ROUTES["/"]);
document.title = route.title;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Suspense fallback={null}>{route.page}</Suspense>
  </StrictMode>,
);
