import { StrictMode } from "react";
import { renderToString } from "react-dom/server";
import { POSTS, toMarkdown } from "./blog/posts";
import { ROUTES, type Route } from "./routes";
import { SITE } from "./site";

/** Used only at build time by scripts/prerender.mjs. */
export { POSTS, ROUTES, SITE, toMarkdown };

export function render(route: Route) {
  return renderToString(<StrictMode>{route.element()}</StrictMode>);
}
