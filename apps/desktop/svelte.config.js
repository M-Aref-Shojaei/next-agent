import adapter from "@sveltejs/adapter-static";

/** @type {import('@sveltejs/kit').Config} */
export default {
  kit: {
    adapter: adapter({ fallback: "index.html" }),
    files: { lib: "src/lib", routes: "src/routes" },
    // Coucou's framework-free TypeScript, vendored whole (ADR-0011); the
    // routes under src/routes/coucou mount its pages.
    alias: { $coucou: "coucou/src" },
  },
};
