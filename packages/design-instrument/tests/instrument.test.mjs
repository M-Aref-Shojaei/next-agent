import { test } from "node:test";
import assert from "node:assert/strict";
import { instrument, instrumentJsx, instrumentMarkup, nextAgentSource } from "../src/index.mjs";

test("JSX host elements get their source position", () => {
  const code = 'export function A({ a, b }) {\n  const t = a < b ? `<p>${"x"}</p>` : "<i>";\n  return (\n    <div className="x">\n      <Card />\n      <span>{a > b && <b>hi</b>}</span>\n    </div>\n  );\n}\n';
  const out = instrumentJsx(code, "src/A.tsx");
  assert.match(out, /<div data-na-src="src\/A\.tsx:4:5" className="x">/);
  assert.match(out, /<span data-na-src="src\/A\.tsx:6:7">/);
  assert.match(out, /<b data-na-src="src\/A\.tsx:6:23">hi/);
  assert.ok(!out.includes("Card data-na-src"), "components are skipped");
  assert.ok(out.includes("`<p>${\"x\"}</p>`") && out.includes('"<i>"'), "strings untouched");
  assert.ok(out.includes("a < b"), "comparisons untouched");
});

test("generics, comments and arrow returns", () => {
  const code = "const xs: Array<string> = [];\n// <div>\nconst C = () => <section/>;\n/* <p> */ function f(){ return <p>x</p> }\n";
  const out = instrumentJsx(code, "a.jsx");
  assert.ok(out.includes("Array<string>"));
  assert.ok(out.includes("// <div>"));
  assert.match(out, /<section data-na-src="a\.jsx:3:17"\/>/);
  assert.match(out, /return <p data-na-src="a\.jsx:4:32">/);
});

test("Svelte markup, skipping script, style and expressions", () => {
  const code = '<script>\n  let n = $state(0); const s = "<div>";\n</script>\n\n<main class="a">\n  {#if n < 2}<p title="a > b">x</p>{/if}\n  <svelte:head></svelte:head>\n  <Comp />\n</main>\n<style>div { color: red }</style>\n';
  const out = instrumentMarkup(code, "src/routes/+page.svelte");
  assert.match(out, /<main data-na-src="src\/routes\/\+page\.svelte:5:1" class="a">/);
  assert.match(out, /<p data-na-src="src\/routes\/\+page\.svelte:6:14" title="a > b">/);
  assert.ok(out.includes('const s = "<div>"'));
  assert.ok(out.includes("<svelte:head>") && out.includes("<Comp />"));
  assert.ok(out.includes("<style>div { color: red }</style>"));
});

test("Vue: only inside the template block", () => {
  const code = "<template>\n  <div><template v-if=\"x\"><b>y</b></template></div>\n</template>\n<script setup>\nconst h = '<i>';\n</script>\n";
  const out = instrument(code, "src/App.vue");
  assert.match(out, /<div data-na-src="src\/App\.vue:2:3">/);
  assert.match(out, /<b data-na-src="src\/App\.vue:2:27">/);
  assert.ok(out.startsWith("<template>\n"));
  assert.ok(out.includes("'<i>'"));
});

test("attribute values are escaped and other files untouched", () => {
  assert.match(instrumentJsx("x = <a/>", 'we"ird.tsx'), /data-na-src="we&quot;ird\.tsx:1:5"/);
  assert.equal(instrument("<div>", "a.ts"), "<div>");
});

test("Vite plugin: dev only, project files only", () => {
  const plugin = nextAgentSource();
  assert.equal(plugin.apply, "serve");
  assert.equal(plugin.enforce, "pre");
  plugin.configResolved({ root: "/p/app/" });
  assert.equal(plugin.transform("x = <a/>", "/p/app/node_modules/x/a.jsx"), null);
  assert.equal(plugin.transform("x = <a/>", "/elsewhere/a.jsx"), null);
  assert.match(plugin.transform("x = <a/>", "/p/app/src/a.jsx?v=1").code, /src\/a\.jsx:1:5/);
});
