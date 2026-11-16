// SketchUp Ruby scripts in SketchCraft: compile with Opal (vendored in
// public/ruby), run against the API in ruby/sketchup.rb, and hand back the
// SketchCraft commands they recorded. Both steps run in sandboxed workers (no
// network, no files); the app replays the commands, so they undo as usual.
import SKETCHUP_RB from "./ruby/sketchup.rb?raw";
import type { Plugin } from "./plugins";

// the same lockdown as JavaScript plugins (see plugins.tsx)
const LOCKDOWN = `
"use strict";
for (const k of ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "indexedDB", "caches"]) {
  try { Object.defineProperty(self, k, { value: undefined, configurable: false }); } catch {}
}
`;

let runtime: Promise<string> | null = null;
let compiler: Promise<string> | null = null;
const text = (url: string) => fetch(url).then((r) => (r.ok ? r.text() : Promise.reject(new Error(`couldn't load ${url}`))));
const opal = () => (runtime ??= text("ruby/opal.js"));
const opalCompiler = () => (compiler ??= text("ruby/opal-compiler.js"));

function worker(code: string, limitMs: number, msg: unknown): Promise<Record<string, unknown>> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(new Blob([code], { type: "text/javascript" }));
    const w = new Worker(url);
    const done = (r: Record<string, unknown>) => {
      clearTimeout(timer);
      w.terminate();
      URL.revokeObjectURL(url);
      resolve(r);
    };
    const timer = setTimeout(() => done({ error: "it took too long, so it was stopped" }), limitMs);
    w.onmessage = (e) => done(e.data ?? {});
    w.onerror = (e) => done({ error: e.message || "it hit an error" });
    w.postMessage(msg);
  });
}

/** Ruby files to JavaScript modules (Opal.modules), by name without .rb. */
async function compile(files: Record<string, string>): Promise<{ js?: string; error?: string }> {
  const [rt, comp] = await Promise.all([opal(), opalCompiler()]);
  const driver = `
self.onmessage = (e) => {
  const out = [];
  try {
    Opal.require("corelib/string/unpack");
    Opal.require("opal/compiler");
    for (const [name, src] of Object.entries(e.data.files)) {
      try {
        const js = Opal.Opal.Compiler.$new(src, Opal.hash({ file: name, requirable: true })).$compile();
        out.push(js);
      } catch (err) {
        postMessage({ error: name + ".rb: " + String((err && (err.message || err.$message && err.$message())) || err) });
        return;
      }
    }
    postMessage({ js: out.join("\\n;\\n") });
  } catch (err) {
    postMessage({ error: "the Ruby compiler didn't start: " + String((err && err.message) || err) });
  }
};`;
  const r = await worker(`${LOCKDOWN}\n${rt}\n;\n${comp}\n;\n${driver}`, 60_000, { files });
  return r as { js?: string; error?: string };
}

// compiled JavaScript is kept between sessions (by a hash of the Ruby), so
// the 2 MB compiler only loads the first time a script is seen
const OPAL = "opal-1.7.3";
function hash(s: string) {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36) + s.length.toString(36);
}
async function cachedCompile(files: Record<string, string>): Promise<{ js?: string; error?: string }> {
  const key = `sketchcraft.rubyjs.${OPAL}.${hash(JSON.stringify(files))}`;
  try {
    const hit = localStorage.getItem(key);
    if (hit) return { js: hit };
  } catch {
    /* no storage: compile every time */
  }
  const r = await compile(files);
  if (r.js) {
    try {
      localStorage.setItem(key, r.js);
    } catch {
      /* full: fine, it compiles again next time */
    }
  }
  return r;
}

const compiled = new Map<string, Promise<{ js?: string; error?: string }>>();
let apiJs: Promise<{ js?: string; error?: string }> | null = null;

/** The plugin's files compiled, once per version of them. */
function compiledPlugin(p: Plugin): Promise<{ js?: string; error?: string }> {
  const files = p.files ?? {};
  const key = `${p.id}:${Object.entries(files)
    .map(([k, v]) => `${k}:${v.length}:${v.slice(0, 64)}`)
    .join("|")}`;
  if (!compiled.has(key)) {
    const named = Object.fromEntries(Object.entries(files).map(([k, v]) => [k.replace(/\.rb$/i, ""), v]));
    compiled.set(key, cachedCompile(named));
  }
  return compiled.get(key)!;
}

export interface RubyRun {
  /** SketchCraft commands to replay, in order */
  journal: Record<string, unknown>[];
  /** the menu items it added (its commands) */
  menu: string[];
  /** what it printed */
  out: string;
  /** messages it showed (UI.messagebox) */
  toasts: string[];
  /** a form it needs filled in (UI.inputbox) before it can finish */
  need?: { prompts: string[]; defaults: string[]; lists: string[]; title: string };
  error?: string;
}

/**
 * Load a Ruby plugin and (if `item` is given) run that menu item, against
 * `answers` for its input boxes so far. Nothing touches the model: the result
 * says what to do.
 */
export async function runRuby(
  p: Plugin,
  opts: { item?: number; answers?: string[][]; selection?: { id: number; name: string; kind: string }[] },
): Promise<RubyRun> {
  const empty = { journal: [], menu: [], out: "", toasts: [] };
  apiJs ??= cachedCompile({ sketchup_api: SKETCHUP_RB });
  const [rt, api, mine] = await Promise.all([opal(), apiJs, compiledPlugin(p)]);
  if (api.error) return { ...empty, error: `SketchCraft's Ruby layer: ${api.error}` };
  if (mine.error) return { ...empty, error: mine.error };
  const entries = (p.entry ?? []).map((e) => e.replace(/\.rb$/i, ""));
  const driver = `
self.onmessage = (e) => {
  const d = e.data;
  globalThis.__runId = d.runId;
  globalThis.__journal = []; globalThis.__menu = []; globalThis.__out = []; globalThis.__toasts = [];
  globalThis.__answers = d.answers; globalThis.__askCount = 0; globalThis.__need = null;
  globalThis.__selection = d.selection;
  const reply = (extra) => postMessage(Object.assign({ journal: globalThis.__journal, menu: globalThis.__menu, out: globalThis.__out.join(""), toasts: globalThis.__toasts }, extra));
  const msg = (err) => {
    if (!err) return "it hit an error";
    const m = err.$message ? err.$message() : err.message;
    const cls = err.$class ? err.$class().$name() : "";
    return (cls && cls !== "RuntimeError" ? cls + ": " : "") + String(m || err);
  };
  try {
    Opal.require("sketchup_api");
    for (const name of d.entries) Opal.require(name);
    if (d.item != null) {
      const items = Opal.UI.$items();
      const it = items[d.item];
      if (!it) return reply({ error: "it has no menu item " + d.item });
      if (it[1] === Opal.nil || !it[1]) return reply({ error: "that menu item does nothing" });
      it[1].$call();
      Opal.SC.$finish();
    }
    reply({});
  } catch (err) {
    if (globalThis.__need) return reply({ need: globalThis.__need, journal: [] });
    reply({ error: msg(err), journal: [] });
  }
};`;
  // Kernel#require finds the plugin's own files by name; a few SketchUp
  // library names are already loaded
  const shims = `
for (const n of ["sketchup", "sketchup.rb", "extensions", "extensions.rb", "langhandler", "langhandler.rb"]) Opal.modules[n] = function () {};
(function () {
  const req = Opal.require;
  Opal.require = function (name) {
    let n = String(name).replace(/\\.(rb|rbe|rbs)$/i, "").replace(/^\\.\\//, "");
    if (Opal.modules[n]) return req.call(Opal, n);
    // relative to a plugin folder: try dropping leading parts
    const parts = n.split("/");
    for (let i = 1; i < parts.length; i++) { const m = parts.slice(i).join("/"); if (Opal.modules[m]) return req.call(Opal, m); }
    for (const m of Object.keys(Opal.modules)) if (m.endsWith("/" + n)) return req.call(Opal, m);
    return req.call(Opal, n);
  };
})();`;
  const r = await worker(`${LOCKDOWN}\n${rt}\n;\n${api.js}\n;\n${mine.js}\n;\n${shims}\n;\n${driver}`, 30_000, {
    runId: Date.now().toString(36),
    entries,
    item: opts.item ?? null,
    answers: opts.answers ?? [],
    selection: opts.selection ?? [],
  });
  return {
    journal: (r.journal as RubyRun["journal"]) ?? [],
    menu: (r.menu as string[]) ?? [],
    out: String(r.out ?? ""),
    toasts: (r.toasts as string[]) ?? [],
    need: r.need as RubyRun["need"],
    error: r.error as string | undefined,
  };
}
