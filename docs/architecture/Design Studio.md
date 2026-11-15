---
title: Design Studio
tags: [architecture, design]
status: draft
---

# Design Studio

The **Design** page in the sidebar. Our own UX. The mechanics come from three Apache-2.0 projects, listed in [[Design Studio Sources]], `NOTICE.md` and `upstream.lock.json`:
- Open Design @ `59e4f2a`
- Onlook @ `423e2e9`
- OpenUI @ `42d7ab4`

## Parts

| Part | Where |
|---|---|
| Mechanics (pure, tested, no UI) | `crates/design` (`next-agent-design`) |
| App side: commands, model choice, tasks, preview state | `apps/desktop/src-tauri/src/design.rs` |
| Storage: table `design_directions` (one row per direction or refinement) | `crates/core/src/store.rs` (`design-studio` block) |
| UI | `apps/desktop/src/routes/design/+page.svelte`, `lib/components/DesignFrame.svelte`, `lib/design.ts` |
| Optional dev-time build plugin | `packages/design-instrument` |

## Flows

1. **Design system.**
   - Each workspace has `DESIGN.md` and `tokens.css`, kept in the workspace root, or in `design/` when the root has no `DESIGN.md`.
   - **Generate from project** reads the project's CSS: stylesheets and `<style>` blocks in Svelte, Vue and HTML. It skips `node_modules`, build output and symlinks. From that it collects custom properties with their dark values, colours, type, spacing, radii and shadows (`system.rs`).
   - For categories with no tokens, it derives tokens from literal values.
   - It writes both files. `DESIGN.md` uses Open Design's nine-section layout.
   - Both files can be edited in the studio. Next Agent's own copies are `DESIGN.md` and `tokens.css` in the repository root (a top-level `design/` folder is reserved by the Coucou asset guard). To regenerate them, run `cargo run -p next-agent-design --example design_system -- apps/desktop/src "Next Agent" .`.
2. **Generate.**
   - The prompt goes to the gateway's `design` role. That role falls back to `chat`, then to the default model.
   - The model returns 3–5 `<artifact>` HTML documents, each in a different posture from Open Design's direction library, all bound to the workspace tokens. A board sketch (`board_sketch`) can be added as a text wireframe.
   - Each direction is linted. Then a self-critique pass scores it on Open Design's five dimensions. When any score is below 3, the reviewer's fixed version replaces it.
   - When no provider is configured, `MockDesignModel` runs the same flows offline.
3. **Pick and refine.**
   - Pick a direction, then refine it by chat.
   - Every change is a new version linked to its parent with `parent_id`. The UI shows the versions as v1, v2 and so on.
4. **Edit the real app.** See the next section.
5. **Convert.**
   - The `design` role turns the HTML into a Svelte, React, Vue or Web Component file. The default framework is detected from `package.json`. The prompt is OpenUI's, adapted.
   - The file lands at `src/lib/components/` (SvelteKit), `src/components/` or `components/`.
   - It is written by a scripted task in a worktree, under a new file name if needed.
6. **Implement this.** Starts an auto-mode agent task. Its prompt holds the direction's HTML and points to `DESIGN.md`.

## Edit mode

```
studio iframe (sandbox APP_SANDBOX)
  └─ http://127.0.0.1:<random>  preview proxy (crates/design/src/proxy.rs, std::net only)
       ├─ /__next_agent/bridge.js   bridge, injected into every HTML page
       └─ → the workspace's dev server on loopback only (localhost, then [::1])
```

1. **Preview.**
   - Leave the URL empty to start the project's `dev` script on a free port. The package manager is chosen from the lockfile, and the dev server runs in its own process group.
   - Or enter a loopback URL. `ssrf::check_preview_url` refuses every other kind of address.
2. **Select.**
   - Click an element, or press Enter on the focused one. The bridge reports where it came from:
     - `data-na-src` from `@next-agent/design-instrument`, for any framework;
     - Svelte dev `__svelte_meta`, zero config;
     - React ≤ 18 `_debugSource`, zero config;
     - Vue `__file`, file only.
   - `source_map::resolve` normalises the path and checks it is a real source file inside the workspace before anything reads it.
3. **Change styles.**
   - The inspector changes layout, spacing, type and fill, and suggests `var(--token)` values.
   - Changes are applied to the preview first, as inline styles that can be reset.
4. **Write to code.**
   - `codemod::apply_style` finds the element's opening tag and writes the change:
     - Svelte: `style:prop` directives;
     - Vue and HTML: a merged static `style="…"`;
     - JSX: a merged `style={{…}}`.
   - Then `AppService::task_start_with` runs a **scripted task**. It requires a worktree, and its single `write_file` call goes through the broker and the permission engine.
   - If the worktree's copy of the file differs from the working copy, `codemod::transplant` re-finds the tag.
5. **Review.** The diff appears in the normal reviewer. Merge, discard or keep it there.

## Security

| Concern | Control |
|---|---|
| Generated HTML | `sandbox=""`: no scripts, forms, popups, navigation or same-origin. Every document starts with our own `<head>`: the no-network CSP `<meta>` (`default-src 'none'`), then the tokens, whose `</style` is escaped in any case. The author's markup comes after it. Every author `<meta http-equiv>`, `<base>`, doctype and prefetch link is stripped, with quoted `>` handled. Lint flags network use and scripts. |
| Live app frame | `allow-scripts allow-same-origin allow-forms` on the **proxy's** origin, never the studio's. No popups or top-level navigation. |
| App CSP | Only `frame-src 'self' http://127.0.0.1:*` was added. A test pins it. |
| Proxy | Listens on 127.0.0.1 only and connects only to its pinned loopback target. It refuses requests whose `Host` is not its own (DNS rebinding), and refuses `CONNECT` and absolute-form requests. Redirects to the dev server are rewritten as relative. |
| Bridge messages | Both directions check `event.source` and the exact origin. The studio origin is validated before it is put into the script. Every message shape is validated (`readBridgeMessage`). |
| Code writes | Location and value checks run in Rust (`check_property`, `check_value`: no quotes, braces, `;`, `<>`, `url(`). Writes happen only in a task worktree, through the broker and permission engine, and are merged after review. |
| Design files | Fixed names only. Symlinks are refused, files are capped at 256 KB, and writes are atomic through an unpredictable temp file opened with `create_new`, which never follows a planted symlink. |

## Not done yet

- **Tailwind:** class-based edits (Onlook's approach) are not done; edits are written as inline styles or directives.
- **Not instrumented:**
  - React 19 needs the instrument plugin.
  - Vue gives only the file without it.
  - Angular, Solid and Astro components are not instrumented.
- **Dev server:** it is stopped when the preview stops or the app exits normally. A crash of the app can leave it running.
- **Previews:** directions don't run scripts, so interactive states are shown statically.
- **Open security items (low, from the Wave 2 review):**
  - Any local web page can send requests to the preview proxy's port: the Host check stops DNS rebinding but not plain cross-site requests, and the proxy drops `X-Frame-Options` for everyone. Fix: a per-session secret path or cookie that the studio's iframe URL carries.
  - The proxy has no connection cap, and its WebSocket tunnel has no idle timeout.
  - The `dev` script runs with the app's full environment. It needs an explicit confirmation in the UI and passes `--strictPort` to Vite, but it is not sandboxed (residual risk R1 in the permission-engine notes).
  - `frame-src http://127.0.0.1:*` lets a frame reach any local port. The direction frames strip every `<meta http-equiv>` and `<base>` and run no scripts, but a link the user clicks inside a direction can still navigate that frame to a local port.
