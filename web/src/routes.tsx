import type { ReactNode } from "react";
import { App } from "./App";
import { Blog } from "./blog/BlogPage";
import { POSTS } from "./blog/posts";
import { Changelog } from "./changelog/ChangelogPage";
import { PAGES } from "./docs/content";
import { Docs } from "./docs/DocsPage";
import { DEFAULT_DESCRIPTION } from "./site";

/** Every page, for the prerender script: path, head metadata and the element to render. */
export type Route = { path: string; title: string; description: string; type?: "website" | "article"; element: () => ReactNode };

export const ROUTES: Route[] = [
  { path: "/", title: "Graft — copy-on-write workspaces for agents", description: DEFAULT_DESCRIPTION, element: () => <App /> },
  {
    path: "/docs",
    title: "Graft docs",
    description: "How to install Graft, create and land grafts, and run agents in them: quickstart, concepts, commands, Blueprints, Radar, Integrate and MCP.",
    element: () => <Docs />,
  },
  ...PAGES.map((p) => ({
    path: `/docs/${p.id}`,
    title: `${p.title} · Graft docs`,
    description: p.summary,
    element: () => <Docs initial={p.id} />,
  })),
  { path: "/changelog", title: "Graft changelog", description: "Everything that has landed in Graft, newest first, linked to its pull request.", element: () => <Changelog /> },
  {
    path: "/blog",
    title: "Graft blog",
    description: "Engineering notes on the decisions behind Graft: filesystems, journals, git internals and the numbers that settled them.",
    element: () => <Blog />,
  },
  ...POSTS.map((p) => ({
    path: `/blog/${p.slug}`,
    title: `${p.title} · Graft blog`,
    description: p.summary,
    type: "article" as const,
    element: () => <Blog slug={p.slug} />,
  })),
];

export function routeFor(pathname: string) {
  const path = pathname.replace(/\/+$/, "") || "/";
  return ROUTES.find((r) => r.path === path) ?? ROUTES[0];
}
