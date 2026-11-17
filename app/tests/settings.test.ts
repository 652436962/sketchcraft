import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { savedSettings } from "../src/skins";
import { cleanEdits, TOOL_INFO, WORKSPACES, withEdits } from "../src/workspace";

// the app reads its settings from localStorage; a plain Map stands in for it
let data: Map<string, string>;
beforeEach(() => {
  data = new Map();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
  };
});
afterEach(() => {
  delete (globalThis as { localStorage?: unknown }).localStorage;
});

const DEFAULTS = { roofKind: "gable", wallHeight: 2.7, columnRound: false, name: "x" };
const ALLOWED = { roofKind: ["gable", "hip", "shed"] } as const;
const read = (raw: string | null) => {
  if (raw !== null) data.set("k", raw);
  return savedSettings("k", DEFAULTS, ALLOWED);
};

describe("savedSettings", () => {
  it("gives the defaults when nothing is saved", () => {
    expect(read(null)).toEqual(DEFAULTS);
  });
  it("keeps good values over the defaults", () => {
    expect(read('{"roofKind":"hip","wallHeight":3,"columnRound":true,"name":"y"}')).toEqual({ roofKind: "hip", wallHeight: 3, columnRound: true, name: "y" });
  });
  it("takes each key on its own", () => {
    expect(read('{"roofKind":"dome","wallHeight":3}')).toEqual({ ...DEFAULTS, wallHeight: 3 });
  });
  it("drops values of the wrong kind", () => {
    expect(read('{"roofKind":3,"wallHeight":"tall","columnRound":"yes","name":null}')).toEqual(DEFAULTS);
  });
  it("drops a choice that is not allowed", () => {
    expect(read('{"roofKind":"dome"}').roofKind).toBe("gable");
  });
  it("drops numbers that are not finite", () => {
    // JSON can't say Infinity, but 1e999 reads as it
    expect(read('{"wallHeight":1e999}').wallHeight).toBe(2.7);
  });
  it("survives anything that is not an object", () => {
    for (const raw of ["null", "[1,2]", "7", '"s"', "true", "{{{", "", "undefined"]) expect(read(raw), raw).toEqual(DEFAULTS);
  });
  it("ignores keys it has no default for", () => {
    expect(read('{"extra":1,"__proto__":{"x":1}}')).toEqual(DEFAULTS);
    expect(Object.hasOwn(read('{"extra":1}'), "extra")).toBe(false);
  });
  it("survives storage that throws", () => {
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: () => {
        throw new Error("blocked");
      },
    };
    expect(savedSettings("k", DEFAULTS)).toEqual(DEFAULTS);
  });
  it("does not change the defaults it was given", () => {
    const d = { a: 1 };
    data.set("k", '{"a":2}');
    expect(savedSettings("k", d)).toEqual({ a: 2 });
    expect(d).toEqual({ a: 1 });
  });
});

describe("cleanEdits (workspace edits read back from storage)", () => {
  it("keeps well-formed edits", () => {
    const e = cleanEdits({ model: { tools: [["select"], ["line", "rect"]], showLeft: false, showRight: true, dock: "classic", leftTab: "views" } });
    expect(e.model).toEqual({ tools: [["select"], ["line", "rect"]], showLeft: false, showRight: true, dock: "classic", leftTab: "views" });
  });
  it("keeps the good parts of a partly bad edit", () => {
    expect(cleanEdits({ arch: { tools: "x", dock: "left", showLeft: "no", leftTab: "nowhere" } })).toEqual({ arch: { dock: "left" } });
  });
  it("rejects tools that do not exist or are not strings", () => {
    expect(cleanEdits({ model: { tools: [["select", "teleport"]] } })).toEqual({});
    expect(cleanEdits({ model: { tools: [[1, 2], [3]] } })).toEqual({});
    expect(cleanEdits({ model: { tools: [["select"], "line"] } })).toEqual({});
    // "place" and "plugin" are not toolbar tools
    expect(cleanEdits({ model: { tools: [["place"]] } })).toEqual({});
  });
  it("limits how many groups and tools it will take", () => {
    expect(cleanEdits({ model: { tools: Array.from({ length: 41 }, () => ["select"]) } })).toEqual({});
    expect(cleanEdits({ model: { tools: [Array.from({ length: 81 }, () => "select")] } })).toEqual({});
    expect(cleanEdits({ model: { tools: Array.from({ length: 40 }, () => ["select"]) } }).model?.tools).toHaveLength(40);
  });
  it("is not fooled by inherited property names", () => {
    expect(cleanEdits({ model: { tools: [["toString"], ["constructor"], ["__proto__"]] } })).toEqual({});
  });
  it("ignores workspaces that do not exist", () => {
    expect(cleanEdits({ nope: { dock: "left" } })).toEqual({});
  });
  it("survives anything that is not an object", () => {
    for (const raw of [null, undefined, 7, "x", true, [1, 2, 3], [], { model: null }, { model: [1] }, { model: "x" }]) expect(cleanEdits(raw), String(raw)).toEqual({});
  });
});

describe("withEdits", () => {
  const model = WORKSPACES[0];
  it("applies edits over the workspace", () => {
    expect(withEdits(model, { showLeft: false }).showLeft).toBe(false);
    expect(withEdits(model, undefined)).toEqual({ ...model, dock: "bottom" });
  });
  it("lets the layout set the dock, and a per-workspace dock override it", () => {
    expect(withEdits(model, undefined, "classic").dock).toBe("classic");
    expect(withEdits(model, undefined, "side").dock).toBe("left");
    expect(withEdits(model, { dock: "bottom" }, "classic").dock).toBe("bottom");
  });
});

describe("the workspaces", () => {
  it("only put real tools on their toolbars", () => {
    for (const w of WORKSPACES) for (const t of w.tools.flat()) expect(Object.hasOwn(TOOL_INFO, t), `${w.id}: ${t}`).toBe(true);
  });
  it("have unique ids and names", () => {
    expect(new Set(WORKSPACES.map((w) => w.id)).size).toBe(WORKSPACES.length);
    expect(new Set(WORKSPACES.map((w) => w.name)).size).toBe(WORKSPACES.length);
  });
  it("give each tool a name, and no two tools one key", () => {
    const keys = Object.values(TOOL_INFO).map((i) => i.key).filter((k) => k && k !== "Space");
    expect(new Set(keys).size).toBe(keys.length);
    for (const [id, i] of Object.entries(TOOL_INFO)) expect(i.name.length, id).toBeGreaterThan(2);
  });
});
