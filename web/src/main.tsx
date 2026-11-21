import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { routeFor } from "./routes";
import "@fontsource-variable/funnel-display/wght.css";
import "@fontsource-variable/geist-mono/wght.css";
import "./styles.css";

// Every route is prerendered to HTML at build time (scripts/prerender.mjs);
// the client takes over that markup. In `npm run dev` there is none, so render fresh.
const route = routeFor(location.pathname);
const container = document.getElementById("root")!;
const app = <StrictMode>{route.element()}</StrictMode>;
if (container.firstElementChild) hydrateRoot(container, app);
else createRoot(container).render(app);
