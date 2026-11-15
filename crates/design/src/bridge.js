// Next Agent Design Studio bridge. Served by the loopback preview proxy and
// injected into the previewed app's HTML. Adapted from Onlook (Apache-2.0,
// © 2024 On Off, Inc.): apps/web/preload/script (element selection and
// style preview inside the frame) @ 423e2e9 — modified: plain script, no
// ids; locations come from data-na-src, Svelte, React or Vue dev metadata;
// style previews are inline styles that the studio can reset.
//
// Trust: this runs inside the user's app. It only reports what was clicked
// and applies style previews the studio asks for; the studio validates
// every location in Rust before reading or editing a file.
(() => {
  "use strict";
  const PARENT = "__NA_PARENT_ORIGIN__";
  if (window.parent === window || window.__nextAgentBridge) return;
  window.__nextAgentBridge = true;

  const STYLE_KEYS = [
    "display", "flexDirection", "justifyContent", "alignItems", "gap",
    "padding", "margin", "width", "height", "color", "backgroundColor",
    "fontSize", "fontWeight", "lineHeight", "borderRadius", "border",
  ];
  let editing = false;
  let selected = null;
  let original = null; // selected element's style attribute before previews
  let box = null;

  const send = (type, data) => window.parent.postMessage({ ns: "next-agent", type, ...data }, PARENT);

  function overlay() {
    if (box && box.isConnected) return box;
    box = document.createElement("div");
    box.setAttribute("data-next-agent-overlay", "");
    box.style.cssText = "position:fixed;pointer-events:none;z-index:2147483647;border:2px solid #3b5bdb;background:rgba(59,91,219,.08);border-radius:2px;display:none;transition:all 60ms";
    document.documentElement.appendChild(box);
    return box;
  }

  function outline(el) {
    const b = overlay();
    if (!el) { b.style.display = "none"; return; }
    const r = el.getBoundingClientRect();
    Object.assign(b.style, { display: "block", left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
  }

  function reactSource(node) {
    const key = Object.keys(node).find((k) => k.startsWith("__reactFiber$") || k.startsWith("__reactInternalInstance$"));
    for (let f = key ? node[key] : null, i = 0; f && i < 20; f = f.return, i++) {
      const s = f._debugSource;
      if (s && s.fileName) return { origin: "react", file: s.fileName, line: s.lineNumber || 0, column: s.columnNumber || 0 };
    }
    return null;
  }

  // Nearest element (self or ancestor) with a known source location.
  function locate(el) {
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
      const tag = n.tagName.toLowerCase();
      const attr = n.getAttribute("data-na-src");
      if (attr) {
        const m = /^(.*):(\d+):(\d+)$/.exec(attr);
        if (m) return { el: n, loc: { origin: "attr", file: m[1], line: +m[2], column: +m[3], tag } };
      }
      const meta = n.__svelte_meta;
      if (meta && meta.loc && meta.loc.file) {
        const v4 = typeof meta.loc.char === "number";
        return { el: n, loc: { origin: v4 ? "svelte4" : "svelte", file: meta.loc.file, line: meta.loc.line, column: meta.loc.column, tag } };
      }
      const react = reactSource(n);
      if (react) return { el: n, loc: { ...react, tag } };
      const vue = n.__vueParentComponent;
      if (vue && vue.type && vue.type.__file) return { el: n, loc: { origin: "vue", file: vue.type.__file, line: 0, column: 0, tag } };
    }
    return null;
  }

  function styles(el) {
    const cs = getComputedStyle(el);
    const out = {};
    for (const k of STYLE_KEYS) out[k] = cs[k];
    return out;
  }

  function select(el) {
    restore();
    const found = locate(el);
    if (!found) { send("nosource", { tag: el.tagName.toLowerCase() }); return; }
    selected = found.el;
    original = selected.getAttribute("style");
    outline(selected);
    const r = selected.getBoundingClientRect();
    send("select", {
      location: found.loc,
      rect: { x: r.x, y: r.y, w: r.width, h: r.height },
      styles: styles(selected),
      text: (selected.textContent || "").trim().slice(0, 80),
    });
  }

  function restore() {
    if (!selected) return;
    if (original === null) selected.removeAttribute("style");
    else selected.setAttribute("style", original);
  }

  const kebab = (p) => p.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());

  addEventListener("message", (e) => {
    if (e.source !== window.parent || e.origin !== PARENT) return;
    const d = e.data;
    if (!d || d.ns !== "next-agent") return;
    if (d.type === "hello") send("ready", { url: location.pathname });
    else if (d.type === "mode") { editing = !!d.edit; if (!editing) { outline(null); } }
    else if (d.type === "style" && selected && Array.isArray(d.changes)) {
      for (const c of d.changes) {
        if (typeof c.property !== "string" || typeof c.value !== "string") continue;
        if (c.value === "") selected.style.removeProperty(kebab(c.property));
        else selected.style.setProperty(kebab(c.property), c.value);
      }
      outline(selected);
      send("styled", { styles: styles(selected) });
    } else if (d.type === "reset") { restore(); outline(selected); if (selected) send("styled", { styles: styles(selected) }); }
    else if (d.type === "parent" && selected && selected.parentElement) select(selected.parentElement);
    else if (d.type === "deselect") { restore(); selected = null; outline(null); }
  });

  addEventListener("mouseover", (e) => { if (editing && !selected) outline(e.target); }, true);
  addEventListener("click", (e) => {
    if (!editing) return;
    e.preventDefault();
    e.stopPropagation();
    select(e.target);
  }, true);
  addEventListener("keydown", (e) => {
    if (!editing) return;
    if (e.key === "Enter" && document.activeElement && document.activeElement !== document.body) {
      e.preventDefault();
      select(document.activeElement);
    } else if (e.key === "Escape") {
      restore(); selected = null; outline(null); send("deselected", {});
    }
  }, true);
  addEventListener("scroll", () => outline(selected), true);
  addEventListener("load", () => send("ready", { url: location.pathname }));
  send("ready", { url: location.pathname });
})();
