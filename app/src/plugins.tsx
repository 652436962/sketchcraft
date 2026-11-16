// Plugins: small scripts that add commands to SketchCraft (see docs/PLUGINS.md).
// Each run gets a fresh Web Worker: no page, no network, no files. Its only
// way out is ctx.run(), and the engine only lets those commands model: draw,
// edit, inspect (core/src/policy.rs: an allowlist, enforced in Rust for
// JavaScript plugins and for SketchUp Ruby alike).
import { useRef, useState } from "react";
import { confirm } from "./ask";
import { exec, type Reply } from "./engine";
import { pickSave } from "./files";
import { LOCKDOWN_SOURCE } from "./lockdown";
import { pluginSandboxHolds } from "./cspcheck";
import { cleanForm, cleanResult, type PluginField, type PluginForm, type PluginResult } from "./pluginclean";

export { cleanForm, cleanResult };
export type { PluginField, PluginForm, PluginResult };
import { formatLength, parseLength } from "./units";

export interface PluginCommand {
  id: string;
  label: string;
  hint?: string;
  /** its icon (a data URL), for the tool bar */
  icon?: string | null;
  /** an interactive tool (sketchcraft.tool), not a one-shot command */
  tool?: boolean;
}

export interface Plugin {
  id: string;
  name: string;
  version?: string;
  author?: string;
  description?: string;
  commands: PluginCommand[];
  code: string;
  builtin: boolean;
  /** off: its commands don't show anywhere */
  enabled?: boolean;
  folder?: string;
  /** a SketchUp Ruby extension: its .rb files (by path) and the ones that load it */
  ruby?: boolean;
  files?: Record<string, string>;
  entry?: string[];
}

export interface PluginList {
  dir: string;
  plugins: Plugin[];
  problems: { id: string; error: string }[];
}

export async function loadPlugins(): Promise<PluginList> {
  const r = await exec({ cmd: "plugins" });
  if (!r.ok) return { dir: "", plugins: [], problems: [] };
  const plugins = ((r.plugins as Plugin[]) ?? []).filter((p) => p && typeof p.code === "string" && Array.isArray(p.commands));
  return { dir: String(r.dir ?? ""), plugins, problems: (r.problems as PluginList["problems"]) ?? [] };
}

/**
 * A first check on what a plugin sends. The real gate is the engine's:
 * commands from plugins run as `execPlugin`, which allows modeling and
 * inspection only (no files, programs, network or plugin management).
 */
export function allowed(cmd: Record<string, unknown>): string | null {
  if (!cmd || typeof cmd !== "object" || Array.isArray(cmd)) return "a command is an object with a cmd";
  if (typeof cmd.cmd !== "string" || !cmd.cmd) return "a command needs a cmd";
  return null;
}

// ---- what a plugin hands the interface -----------------------------------------------
// Plugin code is untrusted, so what it asks the window to show is checked
// and cut down before React sees it (a wrong shape can't blank the app).

/**
 * Always shown above anything a plugin puts on screen, in the app's own words
 * (the plugin can't change it), so a window can't pass for SketchCraft's.
 */
export function PluginBanner({ plugin, form }: { plugin: Plugin; form?: boolean }) {
  return (
    <p className="plugin-banner small" role="note">
      {plugin.builtin ? "From a built-in plugin." : `This ${form ? "form" : "window"} comes from the ${plugin.ruby ? "SketchUp Ruby " : ""}plugin “${plugin.name.slice(0, 60)}”, not from SketchCraft.`}
      {form && !plugin.builtin ? " Never type a password or a key here." : ""}
    </p>
  );
}


// What runs in the worker before the plugin: the sketchcraft API, and no
// way to reach the network or load more code.
const PRELUDE = `
"use strict";
// No network, no more code, and no way to start another worker (a new worker
// would begin without any of this lockdown).
${LOCKDOWN_SOURCE}
const __commands = Object.create(null);
const __tools = Object.create(null);
let __tool = null; // the tool in use: { handlers, ctx }
const __waiting = new Map();
let __rid = 0;
const sketchcraft = Object.freeze({
  /** Add a command: fn(ctx) runs when it's picked. */
  command(id, fn) { __commands[String(id)] = fn; },
  /** Add a tool: { start(ctx), move(ctx, at), click(ctx, at), stop(ctx) }, all optional. */
  tool(id, handlers) { __tools[String(id)] = handlers || {}; },
});
self.onmessage = async (e) => {
  const m = e.data || {};
  if (m.type === "reply") {
    const w = __waiting.get(m.rid);
    __waiting.delete(m.rid);
    if (w) w(m.reply);
    return;
  }
  if (m.type === "tool-start" || m.type === "tool-event") return __toolMessage(m);
  if (m.type !== "invoke") return;
  const fn = __commands[m.id];
  if (typeof fn !== "function") return postMessage({ type: "error", error: "this plugin has no command " + JSON.stringify(m.id) });
  const ctx = Object.freeze({
    selection: m.selection,
    units: m.units,
    run: (cmd) => new Promise((res) => { const rid = ++__rid; __waiting.set(rid, res); postMessage({ type: "exec", rid, cmd: JSON.parse(JSON.stringify(cmd)) }); }),
    show: (r) => postMessage({ type: "show", result: JSON.parse(JSON.stringify(r)) }),
    ask: (form) => new Promise((res) => { const rid = ++__rid; __waiting.set(rid, res); postMessage({ type: "ask", rid, form: JSON.parse(JSON.stringify(form)) }); }),
    toast: (t) => postMessage({ type: "toast", text: String(t) }),
  });
  try { await fn(ctx); postMessage({ type: "done" }); }
  catch (err) { postMessage({ type: "error", error: String((err && err.message) || err) }); }
};
function __toolCtx(m) {
  return Object.freeze({
    selection: m.selection,
    units: m.units,
    run: (cmd) => new Promise((res) => { const rid = ++__rid; __waiting.set(rid, res); postMessage({ type: "exec", rid, cmd: JSON.parse(JSON.stringify(cmd)) }); }),
    ask: (form) => new Promise((res) => { const rid = ++__rid; __waiting.set(rid, res); postMessage({ type: "ask", rid, form: JSON.parse(JSON.stringify(form)) }); }),
    show: (r) => postMessage({ type: "show", result: JSON.parse(JSON.stringify(r)) }),
    toast: (t) => postMessage({ type: "toast", text: String(t) }),
    /** lines to draw while the tool runs: [[from, to], ...] (meters) */
    preview: (lines) => postMessage({ type: "preview", lines: JSON.parse(JSON.stringify(lines || [])) }),
    /** the hint in the corner while the tool runs */
    hint: (text) => postMessage({ type: "hint", text: String(text) }),
    /** a point the cursor's axis locks and rubber band start from (null: none) */
    anchor: (p) => postMessage({ type: "anchor", point: p ? JSON.parse(JSON.stringify(p)) : null }),
    /** leave the tool */
    finish: () => postMessage({ type: "finish" }),
  });
}
async function __toolMessage(m) {
  try {
    if (m.type === "tool-start") {
      const h = __tools[m.id];
      if (!h) return postMessage({ type: "error", error: "this plugin has no tool " + JSON.stringify(m.id) });
      __tool = { h, ctx: __toolCtx(m) };
      if (h.start) await h.start(__tool.ctx);
    } else if (__tool) {
      const f = __tool.h[m.kind];
      if (f) await f(__tool.ctx, m.at);
    }
    postMessage({ type: "handled", seq: m.seq });
  } catch (err) {
    postMessage({ type: "error", error: String((err && err.message) || err) });
  }
}
`;

const LIMIT_MS = 60_000;
/** The most commands one plugin run may send (each is checked, run and counted by the engine). */
export const MAX_COMMANDS = 10_000;

/**
 * Plugin commands run as ONE undo step: the engine's batch opens at the first command
 * and closes when the run (or one tool click) is over, so a single Undo takes back
 * everything the plugin did, however many commands that was.
 */
export function batcher() {
  let open = false;
  return {
    async begin() {
      if (!open) {
        open = true;
        await exec({ cmd: "begin_batch" });
      }
    },
    async end() {
      if (open) {
        open = false;
        await exec({ cmd: "end_batch" });
      }
    },
  };
}

/** Run one plugin command. `run` sends engine commands (and refreshes the model). */
export function runPlugin(
  plugin: Plugin,
  commandId: string,
  env: {
    selection: { faces: number[]; edges: number[]; groups: number[]; instances: number[] };
    units: string;
    run: (cmd: Record<string, unknown>) => Promise<Reply>;
    show: (r: PluginResult, plugin: Plugin) => void;
    toast: (text: string, bad?: boolean) => void;
    /** show a form; the answers, or null if cancelled */
    ask: (form: PluginForm, plugin: Plugin) => Promise<Record<string, unknown> | null>;
    /** the command finished without an error */
    onDone?: () => void;
  },
): Promise<void> {
  return pluginSandboxHolds().then((safe) => new Promise<void>((resolve) => {
    if (!safe.ok) {
      env.toast(`Plugins are off: ${safe.why}.`, true);
      return resolve();
    }
    const url = URL.createObjectURL(new Blob([PRELUDE, "\n;\n", plugin.code], { type: "text/javascript" }));
    let worker: Worker;
    try {
      worker = new Worker(url);
    } catch (e) {
      URL.revokeObjectURL(url);
      env.toast(`${plugin.name} couldn't start: ${String(e)}`, true);
      return resolve();
    }
    const batch = batcher();
    let sent = 0;
    const finish = () => {
      clearTimeout(timer);
      worker.terminate();
      URL.revokeObjectURL(url);
      void batch.end().finally(() => resolve());
    };
    // a minute of running (time spent filling in its forms doesn't count)
    let timer = 0 as unknown as ReturnType<typeof setTimeout>;
    const arm = () => {
      timer = setTimeout(() => {
        env.toast(`${plugin.name} took over a minute, so it was stopped.`, true);
        finish();
      }, LIMIT_MS);
    };
    arm();
    worker.onerror = (e) => {
      env.toast(`${plugin.name}: ${e.message || "it hit an error"}`, true);
      finish();
    };
    worker.onmessage = async (e) => {
      const m = e.data ?? {};
      if (m.type === "exec") {
        let why = allowed(m.cmd);
        if (!why && ++sent > MAX_COMMANDS) why = `a plugin run may send at most ${MAX_COMMANDS} commands`;
        let reply: Reply;
        if (why) reply = { ok: false, error: why };
        else {
          await batch.begin();
          reply = await env.run(m.cmd);
        }
        worker.postMessage({ type: "reply", rid: m.rid, reply });
        if (sent > MAX_COMMANDS) {
          env.toast(`${plugin.name} sent more than ${MAX_COMMANDS} commands, so it was stopped.`, true);
          finish();
        }
      } else if (m.type === "ask") {
        clearTimeout(timer);
        const answers = await env.ask(cleanForm(m.form), plugin);
        arm();
        worker.postMessage({ type: "reply", rid: m.rid, reply: answers });
      } else if (m.type === "show") env.show(cleanResult(m.result), plugin);
      else if (m.type === "toast") env.toast(String(m.text).slice(0, 300));
      else if (m.type === "error") {
        env.toast(`${plugin.name}: ${String(m.error).slice(0, 300)}`, true);
        finish();
      } else if (m.type === "done") {
        env.onDone?.();
        finish();
      }
    };
    worker.postMessage({ type: "invoke", id: commandId, selection: env.selection, units: env.units });
  }));
}

/**
 * One CSV cell. A text cell that starts like a spreadsheet formula (= + - @, tab or
 * return) gets a leading quote, so opening the file in a spreadsheet can't run it;
 * real numbers are left as they are.
 */
export const csvCell = (v: string | number) => {
  let s = String(v ?? "");
  const number = typeof v === "number" && Number.isFinite(v);
  if (!number && /^[=+\-@\t\r]/.test(s) && !/^[-+]?\d+(\.\d+)?([eE][-+]?\d+)?$/.test(s)) s = `'${s}`;
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};

/** What a plugin showed: its text and table, to copy or save as CSV. */
export function PluginResultDialog({ result, plugin, onClose, onToast }: { result: PluginResult; plugin: Plugin; onClose: () => void; onToast: (t: string, bad?: boolean) => void }) {
  const [copied, setCopied] = useState(false);
  const t = result.table;
  const csv = t ? [t.columns, ...t.rows].map((r) => r.map(csvCell).join(",")).join("\n") + "\n" : "";
  const save = async () => {
    const path = await pickSave(`${(result.title ?? plugin.name).replace(/[^\w -]+/g, "").trim() || "plugin"}.csv`, "csv", "CSV");
    if (!path) return;
    const r = await exec({ cmd: "save_text", path, text: csv });
    if (r.ok) onToast(`Saved ${path.split("/").pop()}`);
    else onToast(r.error ?? "Couldn't save it", true);
  };
  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <div className="sheet plugin-result" onClick={(e) => e.stopPropagation()} role="dialog" aria-label={result.title ?? plugin.name}>
        <div className="sheet-head">
          <b>{result.title ?? plugin.name}</b>
          <span className="muted">{plugin.builtin ? "Built-in plugin" : `Plugin: ${plugin.name}${plugin.version ? ` ${plugin.version}` : ""}`}</span>
        </div>
        <PluginBanner plugin={plugin} />
        {result.text && <p className="plugin-text">{result.text}</p>}
        {t && (
          <div className="plugin-table">
            <table>
              <thead>
                <tr>
                  {t.columns.map((c, i) => (
                    <th key={i}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {t.rows.map((r, i) => (
                  <tr key={i}>
                    {r.map((c, j) => (
                      <td key={j}>{String(c)}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="row gap">
          {t && (
            <>
              <button
                className="btn small"
                onClick={() =>
                  navigator.clipboard.writeText([t.columns, ...t.rows].map((r) => r.join("\t")).join("\n")).then(
                    () => setCopied(true),
                    () => onToast("Couldn't copy", true),
                  )
                }
              >
                {copied ? "Copied" : "Copy table"}
              </button>
              <button className="btn small ghost" onClick={save}>
                Save CSV…
              </button>
            </>
          )}
          <button className="btn small ghost" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/** The Plugins window: every plugin, on or off, install from a .zip, remove yours. */
export function PluginsDialog({
  list,
  reload,
  onClose,
  onToast,
  commandsOf,
}: {
  list: PluginList;
  reload: () => Promise<void>;
  onClose: () => void;
  onToast: (t: string, bad?: boolean) => void;
  /** a plugin's commands as found so far (Ruby ones register theirs when they load) */
  commandsOf?: (p: Plugin) => PluginCommand[];
}) {
  const file = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const install = async (f: File | undefined) => {
    if (!f) return;
    setBusy(true);
    const data = await new Promise<string>((res) => {
      const fr = new FileReader();
      fr.onload = () => res(String(fr.result).replace(/^data:[^,]*,/, ""));
      fr.onerror = () => res("");
      fr.readAsDataURL(f);
    });
    // look first: what is it, what does it contain, what would it replace
    const look = await exec({ cmd: "install_plugin", data, name: f.name, preview: true });
    if (!look.ok) {
      setBusy(false);
      return onToast(`Couldn't read ${f.name}: ${look.error}`, true);
    }
    const files = (look.files as { name: string; bytes: number }[]) ?? [];
    const cmds = (look.commands as string[]) ?? [];
    const was = look.installed as { version?: string | null; sha256?: string } | null;
    const replacing = !!was;
    const lines = [
      `${look.kind}${look.version ? ` · version ${look.version}` : ""}${look.author ? ` · by ${look.author}` : ""}`,
      look.description ? String(look.description) : "",
      cmds.length ? `Commands: ${cmds.slice(0, 12).join(", ")}` : "",
      `${files.length} file${files.length === 1 ? "" : "s"}: ${files.slice(0, 8).map((x) => `${x.name} (${x.bytes} B)`).join(", ")}${files.length > 8 ? ", …" : ""}`,
      `Fingerprint (SHA-256): ${String(look.sha256).slice(0, 16)}…`,
      replacing ? `This REPLACES the installed copy (version ${was?.version ?? "unknown"}${was?.sha256 === look.sha256 ? ", identical files" : ", different files"}). The old one is kept in .removed.` : "",
      "Plugins can draw, edit and inspect the model. Install only ones you trust.",
    ].filter(Boolean);
    const yes = await confirm(`${replacing ? "Replace" : "Install"} “${String(look.name).slice(0, 60)}”?`, lines.join("\n"), replacing ? "Replace" : "Install");
    if (!yes) {
      setBusy(false);
      return;
    }
    const r = await exec({ cmd: "install_plugin", data, name: f.name, replace: replacing });
    setBusy(false);
    if (!r.ok) return onToast(`Couldn't install ${f.name}: ${r.error}`, true);
    await reload();
    onToast(`${r.replaced ? "Updated" : "Installed"} ${r.name}. Its commands are in File › Plugins and Shift+S.`);
  };
  const toggle = async (p: Plugin, on: boolean) => {
    const r = await exec({ cmd: "plugin_enable", id: p.id, on });
    if (!r.ok) return onToast(r.error ?? "Couldn't change it", true);
    await reload();
  };
  const remove = async (p: Plugin) => {
    if (!(await confirm(`Remove ${p.name}?`, "It's moved to the .removed folder inside Plugins, so you can put it back.", "Remove"))) return;
    const r = await exec({ cmd: "remove_plugin", id: p.id });
    if (!r.ok) return onToast(r.error ?? "Couldn't remove it", true);
    await reload();
    onToast(`Removed ${p.name}`);
  };
  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <div className="sheet plugins-dlg" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Plugins">
        <div className="sheet-head">
          <b>Plugins</b>
          <span className="muted">Add commands to SketchCraft. They can draw, edit and inspect the model, nothing else: the app refuses any plugin command that touches files, other programs or the network, and what they change can be undone.</span>
        </div>
        <div className="plugin-list">
          {list.plugins.map((p) => (
            <div key={p.id} className={`plugin-row${p.enabled === false ? " off" : ""}`}>
              <label className="plugin-switch" title={p.enabled === false ? "Turn on" : "Turn off"}>
                <input type="checkbox" checked={p.enabled !== false} onChange={(e) => toggle(p, e.target.checked)} aria-label={`${p.name} on`} />
              </label>
              <div className="plugin-about">
                <b>{p.name}</b>
                <span className="muted small">
                  {[p.version && `v${p.version}`, p.author, p.ruby ? "SketchUp Ruby" : null, p.builtin ? "built in" : "yours"].filter(Boolean).join(" · ")}
                </span>
                {p.description && <span className="small">{p.description}</span>}
                <span className="muted small">{(commandsOf?.(p) ?? p.commands).map((c) => c.label).join(", ")}</span>
              </div>
              {!p.builtin && (
                <button className="btn small ghost" onClick={() => remove(p)}>
                  Remove
                </button>
              )}
            </div>
          ))}
          {list.problems.map((p) => (
            <div key={`x${p.id}`} className="plugin-row off">
              <span />
              <div className="plugin-about">
                <b>{p.id}</b>
                <span className="small warn">{p.error}</span>
              </div>
            </div>
          ))}
        </div>
        <input ref={file} type="file" accept=".zip,.rbz,.rb,application/zip" hidden onChange={(e) => (install(e.target.files?.[0]), (e.target.value = ""))} />
        <div className="row gap" style={{ marginTop: 14 }}>
          <button className="btn small" onClick={() => file.current?.click()} disabled={busy}>
            {busy ? "Installing…" : "Install from file…"}
          </button>
          {list.dir && (
            <button className="btn small ghost" onClick={() => exec({ cmd: "reveal", path: list.dir })}>
              Open plugins folder
            </button>
          )}
          <span className="muted small">A SketchCraft plugin (.zip), or a SketchUp Ruby extension (.rbz or .rb).</span>
          <span style={{ flex: 1 }} />
          <button className="btn small" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

/** A length to put in a form field, without losing millimeters (0.018 m, not 0.02 m). */
const exactLength = (m: number, units: string) => (units === "m" || !units ? `${+m.toFixed(4)} m` : formatLength(m, units));

/** A plugin's form: fields from ctx.ask, answers back (lengths in meters). */
export function PluginFormDialog({ form, plugin, units, done }: { form: PluginForm; plugin: Plugin; units: string; done: (answers: Record<string, unknown> | null) => void }) {
  const fields = (form.fields ?? []).filter((f) => f && typeof f.id === "string").slice(0, 40);
  const [vals, setVals] = useState<Record<string, string | boolean>>(() =>
    Object.fromEntries(
      fields.map((f) => [
        f.id,
        f.type === "check" ? !!f.value : f.type === "length" && typeof f.value === "number" ? exactLength(f.value, units) : f.type === "choice" ? String(f.value ?? f.options?.[0] ?? "") : String(f.value ?? ""),
      ]),
    ),
  );
  const [bad, setBad] = useState<string | null>(null);
  const submit = () => {
    const out: Record<string, unknown> = {};
    for (const f of fields) {
      const v = vals[f.id];
      if (f.type === "check") out[f.id] = !!v;
      else if (f.type === "length" || f.type === "number") {
        const n = f.type === "length" ? parseLength(String(v), units) : Number(String(v).trim());
        if (!Number.isFinite(n)) return setBad(`${f.label}: type a ${f.type === "length" ? "length" : "number"}`);
        if (f.min != null && n < f.min) return setBad(`${f.label}: at least ${f.type === "length" ? formatLength(f.min, units) : f.min}`);
        if (f.max != null && n > f.max) return setBad(`${f.label}: at most ${f.type === "length" ? formatLength(f.max, units) : f.max}`);
        out[f.id] = n;
      } else out[f.id] = String(v);
    }
    done(out);
  };
  return (
    <div className="sheet-backdrop" onClick={() => done(null)}>
      <form
        className="sheet plugin-form"
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        onKeyDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-label={form.title ?? plugin.name}
      >
        <div className="sheet-head">
          <b>{form.title ?? plugin.name}</b>
          <span className="muted">{plugin.builtin ? "Built-in plugin" : `Plugin: ${plugin.name}`}</span>
        </div>
        <PluginBanner plugin={plugin} form />
        {form.text && <p className="plugin-text">{form.text}</p>}
        <div className="kv plugin-fields">
          {fields.map((f) =>
            f.type === "check" ? (
              <label key={f.id} className="field check" style={{ gridColumn: "1 / -1" }} title={f.hint}>
                <input type="checkbox" checked={!!vals[f.id]} onChange={(e) => setVals((v) => ({ ...v, [f.id]: e.target.checked }))} />
                <span>{f.label}</span>
              </label>
            ) : (
              <label key={f.id} className="plugin-field" title={f.hint} style={{ display: "contents" }}>
                <span>{f.label}</span>
                {f.type === "choice" ? (
                  <select value={String(vals[f.id])} onChange={(e) => setVals((v) => ({ ...v, [f.id]: e.target.value }))}>
                    {(f.options ?? []).map((o) => (
                      <option key={o} value={o}>
                        {o}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    type="text"
                    inputMode={f.type === "number" ? "decimal" : undefined}
                    value={String(vals[f.id])}
                    placeholder={f.type === "length" ? `e.g. ${exactLength(1, units)}` : undefined}
                    onChange={(e) => setVals((v) => ({ ...v, [f.id]: e.target.value }))}
                  />
                )}
              </label>
            ),
          )}
        </div>
        {bad && <p className="small warn">{bad}</p>}
        <div className="row gap" style={{ marginTop: 14 }}>
          <span style={{ flex: 1 }} />
          <button type="button" className="btn small ghost" onClick={() => done(null)}>
            Cancel
          </button>
          <button type="submit" className="btn small primary">
            {form.ok ?? "OK"}
          </button>
        </div>
      </form>
    </div>
  );
}

/** Where the cursor is, for a plugin tool: the snapped point and what it's on. */
export interface ToolPoint {
  point: [number, number, number];
  /** what it snapped to: endpoint, midpoint, center, origin, on-edge, face, ground, axis… */
  snap: string;
  /** the face under the cursor (its id), if any */
  face: number | null;
  normal: [number, number, number];
  shift: boolean;
  alt: boolean;
}

export interface ToolSession {
  move: (at: ToolPoint) => void;
  click: (at: ToolPoint) => void;
  stop: () => void;
}

const EVENT_LIMIT_MS = 10_000;

/** Start a plugin's tool: a worker that stays while the tool is in use and
 * gets every move and click. Moves are dropped while the last one is busy. */
export function startPluginTool(
  plugin: Plugin,
  toolId: string,
  env: {
    selection: { faces: number[]; edges: number[]; groups: number[]; instances: number[] };
    units: string;
    run: (cmd: Record<string, unknown>) => Promise<Reply>;
    show: (r: PluginResult, plugin: Plugin) => void;
    toast: (text: string, bad?: boolean) => void;
    ask: (form: PluginForm, plugin: Plugin) => Promise<Record<string, unknown> | null>;
    preview: (lines: [number, number, number][][]) => void;
    hint: (text: string) => void;
    anchor: (p: [number, number, number] | null) => void;
    /** the tool ended (finished, failed or stopped) */
    ended: () => void;
  },
): ToolSession {
  const url = URL.createObjectURL(new Blob([PRELUDE, "\n;\n", plugin.code], { type: "text/javascript" }));
  const worker = new Worker(url);
  let seq = 0;
  let busy = false;
  let pendingMove: ToolPoint | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let over = false;
  // one undo step per tool event (a click): the batch opens at its first command and closes when it is handled
  const batch = batcher();
  let sent = 0;
  const end = () => {
    if (over) return;
    over = true;
    void batch.end();
    clearTimeout(timer);
    worker.terminate();
    URL.revokeObjectURL(url);
    env.preview([]);
    env.anchor(null);
    env.ended();
  };
  const arm = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      env.toast(`${plugin.name} took over 10 seconds on one step, so its tool was stopped.`, true);
      end();
    }, EVENT_LIMIT_MS);
  };
  const send = (m: Record<string, unknown>) => {
    if (over) return;
    busy = true;
    arm();
    worker.postMessage({ ...m, seq: ++seq });
  };
  worker.onerror = (e) => {
    env.toast(`${plugin.name}: ${e.message || "it hit an error"}`, true);
    end();
  };
  worker.onmessage = async (e) => {
    const m = e.data ?? {};
    if (m.type === "exec") {
      let why = allowed(m.cmd);
      if (!why && ++sent > MAX_COMMANDS) why = `a plugin tool may send at most ${MAX_COMMANDS} commands`;
      let reply: Reply;
      if (why) reply = { ok: false, error: why };
      else {
        await batch.begin();
        reply = await env.run(m.cmd);
      }
      worker.postMessage({ type: "reply", rid: m.rid, reply });
    } else if (m.type === "ask") {
      clearTimeout(timer);
      const answers = await env.ask(cleanForm(m.form), plugin);
      arm();
      worker.postMessage({ type: "reply", rid: m.rid, reply: answers });
    } else if (m.type === "show") env.show(cleanResult(m.result), plugin);
    else if (m.type === "toast") env.toast(String(m.text).slice(0, 300));
    else if (m.type === "preview") env.preview(Array.isArray(m.lines) ? m.lines.slice(0, 5000) : []);
    else if (m.type === "hint") env.hint(String(m.text).slice(0, 300));
    else if (m.type === "anchor") env.anchor(Array.isArray(m.point) && m.point.length === 3 ? m.point : null);
    else if (m.type === "finish") end();
    else if (m.type === "error") {
      env.toast(`${plugin.name}: ${String(m.error).slice(0, 300)}`, true);
      end();
    } else if (m.type === "handled") {
      busy = false;
      clearTimeout(timer);
      await batch.end();
      if (pendingMove) {
        const at = pendingMove;
        pendingMove = null;
        send({ type: "tool-event", kind: "move", at });
      }
    }
  };
  send({ type: "tool-start", id: toolId, selection: env.selection, units: env.units });
  return {
    move: (at) => {
      if (busy) pendingMove = at;
      else send({ type: "tool-event", kind: "move", at });
    },
    click: (at) => {
      // a click always goes through, after whatever's in flight
      pendingMove = null;
      const go = () => (busy ? setTimeout(go, 15) : send({ type: "tool-event", kind: "click", at }));
      go();
    },
    stop: () => {
      if (over) return;
      // let the plugin tidy up, briefly
      worker.postMessage({ type: "tool-event", kind: "stop", at: null, seq: ++seq });
      setTimeout(end, 150);
    },
  };
}
