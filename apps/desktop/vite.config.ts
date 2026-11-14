import { sveltekit } from "@sveltejs/kit/vite";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const here = dirname(fileURLToPath(import.meta.url));
const FONT_DIR = resolve(here, "node_modules/@excalidraw/excalidraw/dist/prod/fonts");
const URL_BASE = "/excalidraw-assets/fonts/";
// Excalidraw always lists a CDN as the last font source; shipping every font (incl. Xiaolai, ~12 MB) keeps that fallback unused.

function* walk(dir: string): Generator<string> {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) yield* walk(full);
    else yield full;
  }
}

/** Serves (dev) and emits (build) Excalidraw's fonts locally so the CSP never needs a CDN. */
function excalidrawFonts(): Plugin {
  return {
    name: "next-agent-excalidraw-fonts",
    configureServer(server) {
      server.middlewares.use(URL_BASE, (req, res, next) => {
        const file = resolve(FONT_DIR, decodeURIComponent((req.url ?? "").split("?")[0]!).replace(/^\/+/, ""));
        if (!file.startsWith(FONT_DIR) || !existsSync(file) || !statSync(file).isFile()) return next();
        res.setHeader("Content-Type", "font/woff2");
        res.end(readFileSync(file));
      });
    },
    generateBundle() {
      if (this.environment?.name === "ssr" || !existsSync(FONT_DIR)) return; // client build only
      for (const file of walk(FONT_DIR)) {
        this.emitFile({
          type: "asset",
          fileName: `${URL_BASE.slice(1)}${relative(FONT_DIR, file).split("\\").join("/")}`,
          source: readFileSync(file),
        });
      }
    },
  };
}

export default defineConfig({
  plugins: [sveltekit(), excalidrawFonts()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022" },
});
