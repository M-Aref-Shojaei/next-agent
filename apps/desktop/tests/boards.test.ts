import assert from "node:assert/strict";
import { mock, test } from "node:test";
import { createAutosaver } from "../src/lib/boards/autosave.ts";
import { judgeSave } from "../src/lib/boards/guard.ts";
import { mountIsland, type IslandRenderer } from "../src/lib/boards/island.ts";
import { WIREFRAME_LIBRARY } from "../src/lib/boards/library.ts";
import { placeAt, reId, validateSkeletons, type Skeleton } from "../src/lib/boards/skeleton.ts";
import { STICKY_PRESETS, sticky } from "../src/lib/boards/stickies.ts";
import { TEMPLATES } from "../src/lib/boards/templates.ts";

function fakeRenderer() {
  const log: string[] = [];
  const renderer: IslandRenderer<{ theme: string; n: number }> = {
    render: (p) => log.push(`render:${p.theme}:${p.n}`),
    unmount: () => log.push("unmount"),
  };
  return { log, renderer };
}

test("island renders on mount, merges updates, unmounts once", async () => {
  const { log, renderer } = fakeRenderer();
  const island = mountIsland(() => renderer, { theme: "light", n: 1 });
  await island.ready;
  assert.equal(island.mounted, true);
  island.update({ theme: "dark" });
  island.destroy();
  island.destroy();
  island.update({ n: 9 });
  assert.deepEqual(log, ["render:light:1", "render:dark:1", "unmount"]);
  assert.equal(island.mounted, false);
});

test("island destroyed while loading never renders and still unmounts", async () => {
  const { log, renderer } = fakeRenderer();
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  const island = mountIsland(async () => {
    await gate;
    return renderer;
  }, { theme: "light", n: 1 });
  island.update({ theme: "dark" }); // before load: remembered
  island.destroy();
  release();
  await island.ready;
  assert.deepEqual(log, ["unmount"]);
});

test("island updates during load are applied at first render", async () => {
  const { log, renderer } = fakeRenderer();
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  const island = mountIsland(async () => {
    await gate;
    return renderer;
  }, { theme: "light", n: 1 });
  island.update({ theme: "dark" });
  release();
  await island.ready;
  assert.deepEqual(log, ["render:dark:1"]);
});

test("autosave debounces and writes only the latest value", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const saved: number[] = [];
    const statuses: string[] = [];
    const saver = createAutosaver<number>({
      delayMs: 500,
      save: async (v) => void saved.push(v),
      onStatus: (s) => statuses.push(s),
    });
    saver.schedule(() => 1);
    mock.timers.tick(300);
    saver.schedule(() => 2);
    mock.timers.tick(300);
    assert.deepEqual(saved, []);
    mock.timers.tick(300);
    await saver.flush();
    assert.deepEqual(saved, [2]);
    assert.equal(statuses.at(-1), "saved");
    assert.equal(saver.pending, false);
  } finally {
    mock.timers.reset();
  }
});

test("autosave flush saves immediately; cancel drops; errors are reported", async () => {
  const saved: number[] = [];
  let fail = false;
  let lastStatus = "";
  const saver = createAutosaver<number>({
    delayMs: 10_000,
    save: async (v) => {
      if (fail) throw new Error("boom");
      saved.push(v);
    },
    onStatus: (s) => (lastStatus = s),
  });
  saver.schedule(() => 1);
  await saver.flush();
  assert.deepEqual(saved, [1]);
  saver.schedule(() => 2);
  saver.cancel();
  await saver.flush();
  assert.deepEqual(saved, [1]);
  fail = true;
  saver.schedule(() => 3);
  await saver.flush();
  assert.equal(lastStatus, "error");
});

test("library items are valid, uniquely named and non-empty", () => {
  assert.ok(WIREFRAME_LIBRARY.length >= 10);
  const ids = new Set<string>();
  for (const item of WIREFRAME_LIBRARY) {
    assert.ok(!ids.has(item.id), `duplicate library id ${item.id}`);
    ids.add(item.id);
    assert.ok(item.name.length > 0);
    assert.ok(item.elements.length > 0);
    assert.deepEqual(validateSkeletons(item.elements), [], item.id);
  }
});

test("templates are valid; blank is empty; the four requested exist", () => {
  const byId = new Map(TEMPLATES.map((t) => [t.id, t]));
  for (const id of ["blank", "user-flow", "wireframe-page", "retrospective"]) assert.ok(byId.has(id), id);
  assert.equal(byId.get("blank")!.elements.length, 0);
  for (const t of TEMPLATES) assert.deepEqual(validateSkeletons(t.elements), [], t.id);
  const flow = byId.get("user-flow")!.elements;
  assert.ok(flow.filter((e) => e.type === "arrow").length >= 5);
  const retro = byId.get("retrospective")!.elements;
  assert.equal(retro.filter((e) => e.type === "frame").length, 3);
});

test("validator catches broken skeletons", () => {
  const bad: Skeleton[] = [
    { type: "rectangle", id: "a", x: 0, y: 0 },
    { type: "rectangle", id: "a", x: 0, y: 0, width: 10, height: 10 },
    { type: "arrow", x: 0, y: 0, start: { id: "nope" }, end: { id: "a" } },
    { type: "text", x: Number.NaN, y: 0, text: "" },
    { type: "frame", x: 0, y: 0, children: ["zzz"] },
  ];
  const problems = validateSkeletons(bad).join("\n");
  assert.match(problems, /duplicate id a/);
  assert.match(problems, /shape needs width and height/);
  assert.match(problems, /missing id nope/);
  assert.match(problems, /finite/);
  assert.match(problems, /needs text/);
  assert.match(problems, /frame child zzz missing/);
});

test("stickies: preset colours are distinct and a sticky is a labelled square", () => {
  assert.equal(new Set(STICKY_PRESETS.map((p) => p.color)).size, STICKY_PRESETS.length);
  const s = sticky(STICKY_PRESETS[0]!.color, "Hi", 5, 6);
  assert.equal(s.width, s.height);
  assert.equal(s.label?.text, "Hi");
  assert.deepEqual(validateSkeletons([s]), []);
});

test("placeAt centres a group; reId prefixes ids and references", () => {
  const group: Skeleton[] = [
    { type: "rectangle", id: "a", x: 0, y: 0, width: 100, height: 50 },
    { type: "rectangle", id: "b", x: 100, y: 0, width: 100, height: 50 },
    { type: "arrow", x: 0, y: 0, start: { id: "a" }, end: { id: "b" } },
  ];
  const placed = placeAt(group, 1000, 500);
  assert.equal(placed[0]!.x, 900);
  assert.equal(placed[0]!.y, 475);
  const ids = reId(group, "p1-");
  assert.equal(ids[2]!.start?.id, "p1-a");
  assert.deepEqual(validateSkeletons(ids), []);
});

test("flush captures the scene before the canvas is torn down (leave-within-debounce data loss)", async () => {
  // Model of the bug: the canvas reads back empty once unmounted. The getter
  // must run when flush() is called, not after the unmount that follows it.
  const canvas = { mounted: true, elements: ["a", "b"] };
  const read = () => (canvas.mounted ? canvas.elements.length : 0);
  const written: number[] = [];
  const saver = createAutosaver<number>({
    delayMs: 800,
    save: async (n) => {
      await Promise.resolve(); // any await lets teardown run
      written.push(n);
    },
  });
  saver.schedule(read);
  const flushed = saver.flush(); // component teardown: flush first…
  canvas.mounted = false; // …then unmount
  await flushed;
  assert.deepEqual(written, [2]);
});

test("a flush while a save is in flight still captures the pending snapshot immediately", async () => {
  let alive = true;
  const written: (string | null)[] = [];
  let release!: () => void;
  const gate = new Promise<void>((r) => (release = r));
  const saver = createAutosaver<string | null>({
    delayMs: 0,
    save: async (v) => {
      if (written.length === 0) await gate;
      written.push(v);
    },
  });
  saver.schedule(() => "first");
  const first = saver.flush(); // starts the in-flight save, blocked on the gate
  saver.schedule(() => (alive ? "second" : null));
  const second = saver.flush();
  alive = false;
  release();
  await Promise.all([first, second]);
  assert.deepEqual(written, ["first", "second"]);
});

test("save guard refuses an empty scene over content", () => {
  const base = { diskHasContent: true, allowEmpty: false };
  assert.equal(judgeSave(3, base), "save");
  assert.equal(judgeSave(0, base), "empty-held");
  assert.equal(judgeSave(0, { ...base, allowEmpty: true }), "save");
  assert.equal(judgeSave(0, { ...base, diskHasContent: false }), "save");
});

test("a throwing snapshot getter is reported, not left pending", async () => {
  let last = "";
  const saver = createAutosaver<number>({ delayMs: 10_000, save: async () => {}, onStatus: (s) => (last = s) });
  saver.schedule(() => {
    throw new Error("disposed");
  });
  await saver.flush();
  assert.equal(last, "error");
  assert.equal(saver.pending, false);
});
