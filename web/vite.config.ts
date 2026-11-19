import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// A single-page app: /docs and /changelog are client routes (see src/main.tsx).
// Hosts must rewrite unknown paths to /index.html.
export default defineConfig({
  plugins: [react()],
});
