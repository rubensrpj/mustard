import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

/**
 * Where the dev server forwards `/api` — the Rust server this frontend talks
 * to. `MUSTARD_DASHBOARD_PORT` is the same knob the server itself reads, so
 * running it on another port needs the variable set once, in one place.
 */
const apiPort = process.env.MUSTARD_DASHBOARD_PORT || "7777";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  build: {
    // `server::resolve_dist` looks for the built assets at `<crate>/../dist`,
    // i.e. exactly here. Keep the two in step: renaming this directory means
    // the server serves its "assets not found" body instead of the dashboard.
    outDir: "dist",
    emptyOutDir: true,
  },

  server: {
    port: 1420,
    strictPort: true,
    // In `pnpm dev` Vite serves the page and the Rust server answers the data,
    // so the browser's own origin has no `/api`. Proxying the prefix keeps the
    // frontend's URLs relative in both modes — dev and the built bundle the
    // server itself hands out — so no build-time host ever has to be baked in.
    //
    // `/api/events` rides this same rule: the proxy streams the response
    // through rather than buffering it, which is what an open-ended
    // server-sent-events connection needs.
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${apiPort}`,
        changeOrigin: true,
      },
    },
    watch: {
      // The Rust backend lives beside this app; a .rs edit is not an HMR event.
      ignored: ["**/server/**"],
    },
  },
});
