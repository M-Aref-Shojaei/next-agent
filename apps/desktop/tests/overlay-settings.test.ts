// Settings → Overlay's helpers (src/lib/overlay-settings.ts): the routes Coucou's
// page ids land on, and the small rules kept from Coucou's settings window.
import assert from "node:assert/strict";
import test from "node:test";
import {
  OVERLAY_SECTIONS,
  agentDoneText,
  apiState,
  clampAutoClose,
  diffLines,
  errorText,
  exposureNotice,
  keyStoreName,
  overlay,
  overlaySettingsHref,
  relayStateText,
  resolveScreen,
  shortcutTag,
  updateFeedback,
} from "../src/lib/overlay-settings.ts";

test("every page Coucou can ask for lands on a Settings → Overlay route", () => {
  assert.equal(overlaySettingsHref("general"), "/settings/overlay/general");
  assert.equal(overlaySettingsHref("agents"), "/settings/overlay/agents");
  assert.equal(overlaySettingsHref("phone"), "/settings/overlay/phone");
  // Flow's settings and styles, and the Flow pill's "settings".
  assert.equal(overlaySettingsHref("flowsettings"), "/settings/overlay/dictation");
  assert.equal(overlaySettingsHref("settings"), "/settings/overlay/dictation");
  assert.equal(overlaySettingsHref("styles"), "/settings/overlay/dictation#styles");
  // Unknown or missing: General, as Coucou's window fell back to its first page.
  for (const page of ["nope", "", null, undefined]) assert.equal(overlaySettingsHref(page), "/settings/overlay/general");
});

test("each moved page of the Coucou window has a section, and each section a route", () => {
  const sections = new Set<string>(OVERLAY_SECTIONS.map((s) => s.id));
  // Same list as MOVED_PAGES in coucou/src-tauri/src/lib.rs.
  for (const page of ["general", "pills", "agents", "chat", "voice", "integrations", "phone", "shortcuts", "settings", "flowsettings", "styles", "updates", "about"]) {
    const route = overlaySettingsHref(page).split("/").pop()!.split("#")[0];
    assert.ok(sections.has(route), `${page} → ${route}`);
  }
  for (const s of OVERLAY_SECTIONS) assert.equal(overlaySettingsHref(s.id), `/settings/overlay/${s.id}`);
});

test("auto-close stays between 5 and 120 seconds, 15 when unreadable", () => {
  assert.equal(clampAutoClose("30"), 30);
  assert.equal(clampAutoClose("2"), 5);
  assert.equal(clampAutoClose("500"), 120);
  assert.equal(clampAutoClose(""), 15);
  assert.equal(clampAutoClose("abc"), 15);
  assert.equal(clampAutoClose("0"), 15);
});

test("a saved display is found by its key, its place or its name; one that is gone shows as the main one", () => {
  const keys = ["at:0,0|Built-in", "at:1440,0|LG UltraFine"];
  assert.equal(resolveScreen("at:1440,0|LG UltraFine", keys), "at:1440,0|LG UltraFine");
  assert.equal(resolveScreen("at:1440,0|Old name", keys), "at:1440,0|LG UltraFine");
  assert.equal(resolveScreen("at:9,9|LG UltraFine", keys), "at:1440,0|LG UltraFine");
  assert.equal(resolveScreen("cursor", keys), "cursor");
  assert.equal(resolveScreen("primary", keys), "primary");
  assert.equal(resolveScreen("at:5,5|Gone", keys), "primary");
});

test("diff lines are coloured by their first character", () => {
  assert.deepEqual(diffLines(" a\n+b\n-c"), [
    { kind: "ctx", text: " a" },
    { kind: "add", text: "+b" },
    { kind: "del", text: "-c" },
  ]);
});

test("Coucou's words for errors, keys, addresses, shortcuts, updates and the relay", () => {
  assert.equal(errorText(new Error("bad file")), "bad file");
  assert.equal(errorText("plain"), "plain");
  assert.equal(keyStoreName("Mozilla/5.0 (Windows NT 10.0)"), "Windows Credential Manager");
  assert.equal(keyStoreName("Mozilla/5.0 (X11; Linux x86_64)"), "Secret Service (GNOME Keyring, KWallet)");
  assert.equal(apiState(true, false, "Keychain"), "Key saved in the Keychain.");
  assert.match(apiState(false, true, "Keychain"), /subscription/);
  assert.match(apiState(false, false, "Keychain"), /needs one/);

  assert.equal(exposureNotice("local", "http://127.0.0.1:11434", false)?.tone, "hint");
  assert.equal(exposureNotice("remote-http", "http://10.0.0.2", false)?.tone, "hint");
  // A key over plain http to another machine is a warning.
  assert.equal(exposureNotice("remote-http", "http://10.0.0.2", true)?.tone, "warn");
  assert.equal(exposureNotice("invalid", "ftp://x", false)?.tone, "danger");
  assert.equal(exposureNotice("invalid", "  ", false), null);

  assert.deepEqual(shortcutTag(true, { status: "active" }), { tone: "danger", text: "Used twice" });
  assert.equal(shortcutTag(false, { status: "typesCharacter", typed: "@" })?.text, "Types “@”");
  assert.equal(shortcutTag(false, { status: "active", trigger: "Ctrl+Alt+V" })?.text, "Desktop: Ctrl+Alt+V");
  assert.equal(shortcutTag(false, { status: "active" }), null);
  assert.equal(shortcutTag(false, undefined), null);

  assert.equal(updateFeedback({ check: "upToDate" }), "Next Agent is up to date");
  assert.equal(updateFeedback({ check: "unreachable" }), "Couldn't reach GitHub");
  assert.equal(updateFeedback({ check: "checking" }), "");
  assert.equal(relayStateText("linked"), "The phone is connected through the relay.");
  assert.equal(relayStateText("off"), "Off");

  assert.equal(agentDoneText("", ""), "Done. ");
  assert.equal(agentDoneText("a.bak\nb.bak", "Restart Codex."), "Done. Previous version saved as a.bak, b.bak. Restart Codex.");
});

test("outside Tauri the calls answer from the stand-in, with Coucou's error rules", async () => {
  const boot = await overlay.boot();
  assert.equal(typeof boot?.version, "string");
  // A preview carries the fingerprint the write must hand back.
  const preview = await overlay.hooksPreview(true);
  assert.ok(preview.fingerprint && preview.diff.includes("+"));
  assert.equal(typeof (await overlay.hooksApply(true, preview.fingerprint)), "string");
  assert.equal((await overlay.hooksStatus())?.installed, true);
  // Keys: only whether one is stored ever comes back.
  await overlay.secretSet("vercel-token", "secret");
  assert.equal(await overlay.secretPresent("vercel-token"), true);
  await overlay.secretClear("vercel-token");
  assert.equal(await overlay.secretPresent("vercel-token"), false);
  // callOrThrow rejects; call gives null.
  await assert.rejects(overlay.updateSetSource("not a repo"));
  await assert.rejects(overlay.localConnect("ollama", ""));
});
